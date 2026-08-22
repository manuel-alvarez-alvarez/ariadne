//! Keeping the machine awake while agents are working.
//!
//! Agents live in tmux panes driven by long-running CLI processes; if the box
//! sleeps mid-task — the idle timer running out, or a lid coming down — they
//! stall until someone wakes it. The scheduler already knows how many
//! sessions are live on every tick, so it flips this inhibitor on the edges
//! of "any agent working" and the platform keeps the machine up.
//!
//! macOS and Windows go through [`keepawake`], which wraps the two calls we
//! would otherwise hand-roll — `IOPMAssertionCreateWithName` and
//! `SetThreadExecutionState`; the workspace forbids `unsafe_code`, so that FFI
//! has to live in a dependency anyway. Linux needs no FFI at all: logind's
//! `Inhibit` is one D-Bus call, made here directly so the daemon takes the
//! single `sleep:idle` lock the mechanism is meant to be used with.

use tracing::{debug, info, warn};

/// Shown by `pmset -g assertions` / `systemd-inhibit --list`, so it has to
/// explain itself to whoever is wondering why their machine stays up.
const REASON: &str = "Ariadne agents are working";

/// The platform half of the inhibitor: take a system sleep inhibition, and
/// give it back. Kept behind a trait so the edge logic is testable without
/// touching the machine's power state.
///
/// `Sync` because the scheduler that owns the inhibitor is shared across
/// await points on the runtime.
trait Backend: Send + Sync {
    fn acquire(&mut self) -> anyhow::Result<()>;
    fn release(&mut self);
}

/// Holds a system sleep inhibition while `set_active(true)` is in force.
///
/// Idempotent by construction: the platform is only called on a transition,
/// so repeated calls with the same value neither stack inhibitions nor
/// double-release. A failed acquisition leaves the inhibitor inactive, so the
/// next tick retries instead of pretending the machine is pinned awake.
pub struct SleepInhibitor {
    backend: Box<dyn Backend>,
    active: bool,
    /// Whether the current run of failures has already been reported: a
    /// platform with no inhibition mechanism at all must not warn every tick.
    warned: bool,
}

impl SleepInhibitor {
    pub fn new() -> Self {
        Self::with_backend(platform_backend())
    }

    fn with_backend(backend: Box<dyn Backend>) -> Self {
        Self {
            backend,
            active: false,
            warned: false,
        }
    }

    /// Acquire on the false→true edge, release on the true→false edge.
    pub fn set_active(&mut self, active: bool) {
        if active == self.active {
            return;
        }
        if active {
            match self.backend.acquire() {
                Ok(()) => {
                    info!(reason = REASON, "system sleep inhibited");
                    self.active = true;
                    self.warned = false;
                }
                // Deliberately stays inactive: scheduling must not depend on
                // power management, and the next edge gets another try.
                Err(e) => {
                    let e = format!("{e:#}");
                    if self.warned {
                        debug!(error = %e, "inhibiting system sleep failed again");
                    } else {
                        warn!(error = %e, "inhibiting system sleep failed; the machine may sleep while agents work");
                        self.warned = true;
                    }
                }
            }
        } else {
            self.backend.release();
            self.active = false;
            info!("system sleep inhibition released");
        }
    }
}

impl Default for SleepInhibitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SleepInhibitor {
    fn drop(&mut self) {
        if self.active {
            self.backend.release();
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
fn platform_backend() -> Box<dyn Backend> {
    Box::new(native::NativeBackend::default())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn platform_backend() -> Box<dyn Backend> {
    Box::new(UnsupportedBackend)
}

/// Platforms with no inhibition mechanism we know of: refusing to acquire
/// makes the inhibitor a no-op and gets the "sleep may happen" warning logged
/// once.
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
struct UnsupportedBackend;

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
impl Backend for UnsupportedBackend {
    fn acquire(&mut self) -> anyhow::Result<()> {
        anyhow::bail!("no sleep inhibition backend for this platform")
    }
    fn release(&mut self) {}
}

/// macOS and Windows: one `keepawake` handle, released when it drops.
#[cfg(any(target_os = "macos", windows))]
mod platform {
    /// What holding the inhibition amounts to on this platform.
    pub(super) type Held = keepawake::KeepAwake;

    pub(super) fn create() -> anyhow::Result<Held> {
        let mut builder = keepawake::Builder::default();
        builder
            // macOS: an `IOPMAssertionCreateWithName` assertion of type
            // PreventUserIdleSystemSleep. Windows:
            // `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`.
            // Both stop the idle timer; neither touches the display, which
            // may sleep while the agents keep working.
            .idle(true)
            .reason(super::REASON);
        // macOS only: PreventUserIdleSystemSleep holds off the *idle timer*
        // and nothing else, so closing the lid still put the machine to
        // sleep on a working agent ("Entering Sleep state due to 'Clamshell
        // Sleep'" in `pmset -g log`). PreventSystemSleep — what `caffeinate
        // -s` takes — is the assertion that covers that, and the display is
        // still free to sleep under it. Apple honours it on AC power only:
        // on battery the lid still wins, which no assertion can change.
        //
        // Not on Windows, where the same flag means ES_AWAYMODE_REQUIRED —
        // away mode, a different feature that modern standby machines
        // largely ignore.
        #[cfg(target_os = "macos")]
        builder.sleep(true);
        Ok(builder.create()?)
    }
}

/// Linux: one systemd-logind inhibitor lock, held as the fd logind handed us
/// and released by closing it.
#[cfg(target_os = "linux")]
mod platform {
    use zbus::blocking::Connection;
    use zbus::zvariant::OwnedFd;

    /// The slice of `org.freedesktop.login1.Manager` we need.
    #[zbus::proxy(
        interface = "org.freedesktop.login1.Manager",
        default_service = "org.freedesktop.login1",
        default_path = "/org/freedesktop/login1"
    )]
    trait Manager {
        fn inhibit(&self, what: &str, who: &str, why: &str, mode: &str) -> zbus::Result<OwnedFd>;
    }

    /// The lock lives exactly as long as the fd: logind releases it the
    /// moment the last copy is closed, which dropping this does. The
    /// connection rides along so the lock does not depend on a bus
    /// connection nobody owns any more.
    pub(super) struct Held {
        _lock: OwnedFd,
        _conn: Connection,
    }

    pub(super) fn create() -> anyhow::Result<Held> {
        // A machine with no logind (or no D-Bus at all) fails here, which the
        // caller turns into one warning and a no-op inhibitor.
        let conn = Connection::system()?;
        let manager = ManagerProxyBlocking::new(&conn)?;
        // One lock for both: "sleep" alone would not stop the idle timer, and
        // "idle" alone would not stop a suspend the system decides on. Mode
        // "block" refuses the sleep outright rather than asking for a delay.
        // "ariadned" is the `who`, which is what `systemd-inhibit --list`
        // shows next to the reason.
        let lock = manager.inhibit("sleep:idle", "ariadned", super::REASON, "block")?;
        Ok(Held {
            _lock: lock,
            _conn: conn,
        })
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
mod native {
    use std::sync::mpsc;

    use super::Backend;
    use super::platform::{Held, create};

    /// The inhibition is held on a dedicated thread of its own.
    ///
    /// Two reasons: Windows' `SetThreadExecutionState` is per-thread, so an
    /// inhibition taken on one tokio worker and dropped on another would leak
    /// the first thread's execution state forever; and Linux talks blocking
    /// D-Bus, which has no business running on a runtime worker.
    #[derive(Default)]
    pub(super) struct NativeBackend {
        worker: Option<Worker>,
    }

    struct Worker {
        tx: mpsc::Sender<Cmd>,
        handle: std::thread::JoinHandle<()>,
    }

    enum Cmd {
        /// Take the inhibition; reply carries the platform's verdict.
        Acquire(mpsc::Sender<Result<(), String>>),
        /// Drop it; the reply makes the release ordered against our caller.
        Release(mpsc::Sender<()>),
    }

    fn run(rx: mpsc::Receiver<Cmd>) {
        let mut held: Option<Held> = None;
        while let Ok(cmd) = rx.recv() {
            match cmd {
                Cmd::Acquire(reply) => {
                    let result = create().map(|inhibition| held = Some(inhibition));
                    let _ = reply.send(result.map_err(|e| format!("{e:#}")));
                }
                Cmd::Release(reply) => {
                    held = None;
                    let _ = reply.send(());
                }
            }
        }
    }

    impl NativeBackend {
        /// The worker is spawned on first use and outlives individual
        /// acquisitions, so the execution state Windows tracks per thread
        /// always belongs to the same thread.
        fn worker(&mut self) -> &Worker {
            self.worker.get_or_insert_with(|| {
                let (tx, rx) = mpsc::channel();
                let handle = std::thread::Builder::new()
                    .name("sleep-inhibitor".to_string())
                    .spawn(move || run(rx))
                    .expect("spawning the sleep inhibitor thread");
                Worker { tx, handle }
            })
        }
    }

    impl Backend for NativeBackend {
        fn acquire(&mut self) -> anyhow::Result<()> {
            let (reply_tx, reply_rx) = mpsc::channel();
            let worker = self.worker();
            worker.tx.send(Cmd::Acquire(reply_tx))?;
            reply_rx.recv()?.map_err(|e| anyhow::anyhow!(e))
        }

        fn release(&mut self) {
            let Some(worker) = self.worker.as_ref() else {
                return;
            };
            let (reply_tx, reply_rx) = mpsc::channel();
            if worker.tx.send(Cmd::Release(reply_tx)).is_ok() {
                let _ = reply_rx.recv();
            }
        }
    }

    impl Drop for NativeBackend {
        fn drop(&mut self) {
            // Closing the channel ends the loop, which drops the inhibition;
            // joining keeps that release inside the daemon's lifetime.
            if let Some(worker) = self.worker.take() {
                drop(worker.tx);
                let _ = worker.handle.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Log {
        acquires: usize,
        releases: usize,
        /// Number of leading acquisitions that fail.
        failures: usize,
    }

    #[derive(Clone, Default)]
    struct FakeBackend(Arc<Mutex<Log>>);

    impl Backend for FakeBackend {
        fn acquire(&mut self) -> anyhow::Result<()> {
            let mut log = self.0.lock().unwrap();
            log.acquires += 1;
            if log.failures > 0 {
                log.failures -= 1;
                anyhow::bail!("no power management here");
            }
            Ok(())
        }

        fn release(&mut self) {
            self.0.lock().unwrap().releases += 1;
        }
    }

    fn with_fake(fake: &FakeBackend) -> SleepInhibitor {
        SleepInhibitor::with_backend(Box::new(fake.clone()))
    }

    /// (acquires, releases) seen by the platform so far.
    fn counts(fake: &FakeBackend) -> (usize, usize) {
        let log = fake.0.lock().unwrap();
        (log.acquires, log.releases)
    }

    #[test]
    fn only_the_edges_reach_the_platform() {
        let fake = FakeBackend::default();
        let mut inhibitor = with_fake(&fake);

        // A daemon that never has agents never releases what it does not hold.
        inhibitor.set_active(false);
        assert_eq!((0, 0), counts(&fake));

        inhibitor.set_active(true);
        inhibitor.set_active(true);
        inhibitor.set_active(true);
        assert_eq!((1, 0), counts(&fake));

        inhibitor.set_active(false);
        inhibitor.set_active(false);
        assert_eq!((1, 1), counts(&fake));

        inhibitor.set_active(true);
        assert_eq!((2, 1), counts(&fake));
    }

    #[test]
    fn dropping_while_active_releases_once() {
        let fake = FakeBackend::default();
        let mut inhibitor = with_fake(&fake);
        inhibitor.set_active(true);
        drop(inhibitor);
        assert_eq!((1, 1), counts(&fake));

        // ... and an inhibitor that already released does not release again.
        let mut inhibitor = with_fake(&fake);
        inhibitor.set_active(true);
        inhibitor.set_active(false);
        drop(inhibitor);
        assert_eq!((2, 2), counts(&fake));
    }

    #[test]
    fn a_failed_acquisition_is_retried_and_never_released() {
        let fake = FakeBackend::default();
        fake.0.lock().unwrap().failures = 3;
        let mut inhibitor = with_fake(&fake);

        // Every tick with agents working tries again, instead of poisoning
        // the inhibitor on the first failure.
        for expected in 1..=3 {
            inhibitor.set_active(true);
            assert_eq!((expected, 0), counts(&fake));
        }

        // Nothing was ever taken, so going idle — and dropping — releases
        // nothing.
        inhibitor.set_active(false);
        drop(inhibitor);
        assert_eq!((3, 0), counts(&fake));
    }
}
