//! `ariadne setup` — the one-time, interactive host preparation that a daemon
//! cannot do for itself.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};

/// `ariadne setup codex-hooks` — have the user trust Ariadne's Codex hooks.
///
/// Nothing is installed: the hooks travel with every spawned session as `-c`
/// overrides ([`ariadne_core::codex_hooks`]). What is missing on a fresh
/// machine is codex's *trust* in them, which only a human can grant and only
/// at the start of a session. So this opens one, carrying exactly the flags
/// the daemon will spawn with — codex asks, the user answers once, and the
/// verdict is stored under a key with no worktree or session in it, so every
/// later session is covered.
pub fn codex_hooks(cli_bin: Option<String>) -> Result<()> {
    let cli_bin = cli_bin.unwrap_or_else(default_cli_bin);
    let flags = ariadne_core::codex_hooks::config_flags(&cli_bin);

    println!("Codex hooks report session ids and liveness to Ariadne:");
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

    if which("codex").is_none() {
        println!(
            "\ncodex is not on PATH. Install it, then run this command again — \
             until then\ncodex sessions would run without reporting anything back."
        );
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        println!("\nNot a terminal, so the prompt cannot be answered here. Run:");
        println!("  ariadne setup codex-hooks");
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
        println!("\ncodex exited with {status}; the hooks may not be trusted yet.");
        return Ok(());
    }
    println!("\nDone. Ariadne's codex sessions will report from now on.");
    Ok(())
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

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|c| c.is_file())
}
