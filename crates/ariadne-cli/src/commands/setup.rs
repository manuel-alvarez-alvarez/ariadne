//! `ariadne setup` — the one-time, interactive host preparation that a daemon
//! cannot do for itself.

use std::io::IsTerminal;
use std::process::Command;

use anyhow::{Context, Result};

use crate::codex_trust::Trust;
use crate::commands::on_path;
use crate::output::{style, view};

/// `ariadne setup codex-hooks` — have the user trust Ariadne's Codex hooks.
///
/// Nothing is installed: the hooks travel with every spawned session as `-c`
/// overrides ([`ariadne_core::codex_hooks`]). What is missing on a fresh
/// machine is codex's *trust* in them, which only a human can grant and only
/// at the start of a session. So this opens one, carrying exactly the flags
/// the daemon will spawn with; the verdict is stored under a key with no
/// worktree or session in it, so every later session is covered.
pub fn codex_hooks(cli_bin: Option<String>) -> Result<()> {
    let cli_bin = cli_bin.unwrap_or_else(default_cli_bin);
    let flags = ariadne_core::codex_hooks::config_flags(&cli_bin);

    println!(
        "{}",
        style::paint(
            view().color,
            style::HEADING,
            "Codex hooks report session ids, liveness and approval waits to Ariadne:"
        )
    );
    println!(
        "  command: {}",
        ariadne_core::codex_hooks::command(&cli_bin)
    );
    println!(
        "  events:  {}",
        ariadne_core::codex_hooks::EVENTS.join(", ")
    );
    println!(
        "\nThey are passed to every session Ariadne spawns, but codex runs them \
         only\nonce you have trusted them — and it asks at the start of a \
         session."
    );

    // Trust is per event, so an Ariadne that declares one the last run of
    // this command did not leaves the rest working and that one silent.
    if let Some(before) = trust() {
        report_trust(&before);
    }

    if on_path("codex").is_none() {
        println!(
            "\ncodex is not on PATH. Install it, then run this command again — \
             until then\ncodex sessions would run without reporting anything back."
        );
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        println!("\nNot a terminal, so the prompt cannot be answered here. Run:");
        println!(
            "  {}",
            style::paint(view().color, style::TITLE, "ariadne setup codex-hooks")
        );
        return Ok(());
    }

    println!(
        "\nStarting codex now. Answer \"Trust all and continue\" (reviewing them \
         first is\nfine — every entry runs the same command above), then leave \
         with /quit.\nNothing else is needed, and the trust survives every later \
         session."
    );
    print!("\nPress Enter to start codex (Ctrl-C to skip)... ");
    flush();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .context("reading confirmation")?;

    let status = Command::new("codex")
        .args(&flags)
        .status()
        .context("running codex")?;
    if !status.success() {
        println!(
            "\n{}",
            style::paint(
                view().color,
                style::WARN,
                &format!("codex exited with {status}; the hooks may not be trusted yet.")
            )
        );
        return Ok(());
    }

    // Codex records the verdict as it takes it, so its own config is the only
    // honest answer — "done" on an exit status alone is how a half-trusted
    // install goes unnoticed.
    match trust() {
        Some(after) if after.is_complete() => {
            println!(
                "\n{}",
                style::paint(
                    view().color,
                    style::OK,
                    "Done. Ariadne's codex sessions will report from now on."
                )
            );
        }
        Some(after) => {
            println!(
                "\ncodex still has no verdict for: {}",
                after.untrusted_keys().join(", ")
            );
            println!(
                "Every session will stop on the same prompt until it does. Run \
                 this command\nagain and answer \"Trust all and continue\"."
            );
        }
        None => println!(
            "\n{}",
            style::paint(
                view().color,
                style::OK,
                "Done, as far as can be told — codex's home was not found."
            )
        ),
    }
    Ok(())
}

/// What codex records of the declaration, or nothing when its home is not
/// where it can be found.
fn trust() -> Option<Trust> {
    crate::codex_trust::codex_home().map(|home| Trust::read(&home))
}

/// The verdicts as they stand, in the words that say what to do about them.
fn report_trust(trust: &Trust) {
    if trust.is_complete() {
        println!("\nCodex already trusts all of them; this will simply confirm it.");
    } else if trust.is_stale() {
        println!(
            "\nCodex trusts {} of them already and has never been asked about: {}",
            trust.trusted.len(),
            trust.untrusted_keys().join(", ")
        );
    } else {
        println!("\nCodex trusts none of them yet.");
    }
}

fn flush() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

/// The `ariadne` the hook command points at. Must be the same binary the
/// daemon spawns agents with (its `cli_bin`), which by default — and after
/// `install.sh` — is this one.
fn default_cli_bin() -> String {
    std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "ariadne".to_string())
}
