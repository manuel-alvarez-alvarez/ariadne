//! `ariadne completions ...` — shell completion, printed or installed.
//!
//! What is printed is the *dynamic* registration: a few lines of shell that
//! call `ariadne` back on every TAB, so ids, profile names and models are the
//! ones the daemon has right now rather than a snapshot taken at install
//! time. It is byte for byte what `COMPLETE=<shell> ariadne` prints — the
//! same writer, addressed the same way — because the discoverable command and
//! the one the installer wires up must not drift apart.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Subcommand, ValueEnum};
use clap_complete::Shell;
use clap_complete::env::Shells;

use crate::output::note;

/// The environment variable the registration hands back to us on TAB. It is
/// clap's default and the installed rc lines name it literally, so it is
/// spelled once, here.
const VAR: &str = "COMPLETE";

/// The markers around the block `completions install` owns in an rc file.
/// `scripts/uninstall.sh` knows the same two lines: stripping them is how it
/// takes the completions back out.
const OPEN: &str = "# >>> ariadne >>>";
const CLOSE: &str = "# <<< ariadne <<<";

#[derive(Subcommand)]
pub enum CompletionsCommand {
    /// Add the registration to the shell's startup file
    ///
    /// Writes a marked block that sources the completions on every new
    /// shell, so they are regenerated rather than frozen. Running it again
    /// replaces that block instead of adding a second one.
    Install {
        /// Shell to install for (default: what $SHELL names)
        #[arg(long)]
        shell: Option<InstallShell>,
    },
}

/// The shells `completions install` knows a startup file for. Every shell
/// clap can complete is still printable — this is only about where the line
/// goes.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InstallShell {
    Bash,
    Zsh,
    Fish,
}

pub fn run(
    shell: Option<Shell>,
    static_script: bool,
    command: Option<CompletionsCommand>,
) -> Result<()> {
    match (command, shell) {
        (Some(CompletionsCommand::Install { shell }), _) => install(shell),
        (None, Some(shell)) if static_script => {
            clap_complete::generate(
                shell,
                &mut crate::cli::command(),
                "ariadne",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        (None, Some(shell)) => print_registration(shell),
        // clap refuses the shell-less form itself (`required_unless_present`).
        (None, None) => unreachable!(),
    }
}

/// Print what `COMPLETE=<shell> ariadne` prints.
fn print_registration(shell: Shell) -> Result<()> {
    let mut out = Vec::new();
    write_registration(shell, &completer(), &mut out)?;
    std::io::stdout().write_all(&out)?;
    Ok(())
}

/// The dynamic registration for one shell, as clap's own completer writes it.
fn write_registration(shell: Shell, completer: &str, out: &mut dyn Write) -> Result<()> {
    let name = shell.to_string();
    let shells = Shells::builtins();
    let writer = shells
        .completer(&name)
        .with_context(|| format!("no dynamic completion for {name}"))?;
    writer.write_registration(VAR, "ariadne", "ariadne", completer, out)?;
    Ok(())
}

/// The binary the registration calls back on TAB, resolved the way
/// `CompleteEnv` resolves it: argv[0] as typed, made absolute when it was a
/// path rather than a bare name on `PATH`.
fn completer() -> String {
    let argv0 = std::env::args_os().next().unwrap_or_default();
    let mut path = PathBuf::from(argv0);
    if path.components().count() > 1
        && let Ok(cwd) = std::env::current_dir()
    {
        path = cwd.join(path);
    }
    path.to_string_lossy().into_owned()
}

// ---- install -------------------------------------------------------------

/// Write the block into the shell's startup file, and say what happened.
fn install(shell: Option<InstallShell>) -> Result<()> {
    let shell = match shell {
        Some(shell) => shell,
        None => from_env().context(
            "cannot tell which shell this is from $SHELL — name one: \
             ariadne completions install --shell zsh",
        )?,
    };
    let file = startup_file(shell)?;
    let block = block(shell, &installed_binary());
    let changed = write_block(&file, &block)?;
    match changed {
        true => note(&format!("completions installed in {}", file.display())),
        false => note(&format!(
            "completions already installed in {}",
            file.display()
        )),
    }
    note("open a new shell to pick them up");
    Ok(())
}

/// The shell `$SHELL` names, when it is one we know a startup file for.
fn from_env() -> Option<InstallShell> {
    let shell = std::env::var_os("SHELL")?;
    let name = Path::new(&shell)
        .file_stem()?
        .to_string_lossy()
        .into_owned();
    InstallShell::from_str(&name, true).ok()
}

/// Where each shell reads its startup lines from. zsh honours `ZDOTDIR`, as
/// zsh itself does; fish takes a completions file of its own rather than a
/// line in its config.
fn startup_file(shell: InstallShell) -> Result<PathBuf> {
    let home = dirs::home_dir().context("no home directory to install into")?;
    Ok(match shell {
        InstallShell::Bash => home.join(".bashrc"),
        // `${ZDOTDIR:-$HOME}`, empty-is-unset included: an exported but
        // empty ZDOTDIR is what zsh itself ignores, and joining it would put
        // the block in the working directory.
        InstallShell::Zsh => match std::env::var_os("ZDOTDIR").filter(|d| !d.is_empty()) {
            Some(dir) => PathBuf::from(dir).join(".zshrc"),
            None => home.join(".zshrc"),
        },
        InstallShell::Fish => home.join(".config/fish/completions/ariadne.fish"),
    })
}

/// The binary the installed line runs. Unlike the printed registration this
/// outlives the shell that wrote it, so it names the executable itself rather
/// than however it was typed.
fn installed_binary() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "ariadne".to_string())
}

/// The block, between its markers. `scripts/install.sh` used to write these
/// lines itself; it now calls this command, so there is one of them.
fn block(shell: InstallShell, bin: &str) -> String {
    let body = match shell {
        InstallShell::Bash => {
            format!("[ -x \"{bin}\" ] && source <({VAR}=bash \"{bin}\")\n")
        }
        // compdef only exists after compinit; the guard keeps shells without
        // compsys working.
        InstallShell::Zsh => format!(
            "if [ -x \"{bin}\" ] && (( $+functions[compdef] )); then\n    \
             source <({VAR}=zsh \"{bin}\")\nfi\n"
        ),
        InstallShell::Fish => {
            format!("if test -x \"{bin}\"\n    {VAR}=fish \"{bin}\" | source\nend\n")
        }
    };
    format!("{OPEN}\n{body}{CLOSE}\n")
}

/// Put `block` in `file`: in place of the block already there, else appended.
/// `Ok(false)` means the file already said exactly this and was left alone —
/// running the command twice is running it once.
fn write_block(file: &Path, block: &str) -> Result<bool> {
    let existing = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
    };
    let wanted = replace_block(&existing, block);
    if wanted == existing {
        return Ok(false);
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(file, &wanted).with_context(|| format!("writing {}", file.display()))?;
    Ok(true)
}

/// `text` with its ariadne block replaced by `block`, or `block` appended
/// when there is none. An opening marker with no closing one takes the rest
/// of the file with it — that is a block someone truncated, not rc lines to
/// keep.
fn replace_block(text: &str, block: &str) -> String {
    let Some(open) = text.find(OPEN) else {
        let separator = match text.is_empty() || text.ends_with('\n') {
            true => "",
            false => "\n",
        };
        return format!("{text}{separator}{block}");
    };
    let tail = match text[open..].find(CLOSE) {
        Some(close) => {
            let end = open + close + CLOSE.len();
            text[end..].strip_prefix('\n').unwrap_or(&text[end..])
        }
        None => "",
    };
    format!("{}{block}{tail}", &text[..open])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The printed registration is the shim's own, so a shell sourcing it
    /// gets the live candidates and not a snapshot.
    #[test]
    fn the_printed_registration_calls_the_binary_back() {
        let mut out = Vec::new();
        write_registration(Shell::Zsh, "/usr/local/bin/ariadne", &mut out).expect("zsh");
        let script = String::from_utf8(out).expect("utf-8");
        assert!(script.contains("COMPLETE="), "{script}");
        assert!(script.contains("/usr/local/bin/ariadne"), "{script}");
        assert!(
            script.contains("compdef"),
            "zsh registers with compdef: {script}"
        );
    }

    /// Every shell the value enum offers has a registration to print.
    #[test]
    fn every_shell_can_be_printed() {
        for shell in Shell::value_variants() {
            let mut out = Vec::new();
            write_registration(*shell, "ariadne", &mut out)
                .unwrap_or_else(|e| panic!("{shell}: {e}"));
            assert!(!out.is_empty(), "{shell} printed nothing");
        }
    }

    /// The installed block is the one `scripts/install.sh` used to write by
    /// hand: same markers, same compdef guard, same `-x` test.
    #[test]
    fn the_installed_block_is_the_one_the_installer_wrote() {
        let zsh = block(InstallShell::Zsh, "/home/u/.local/bin/ariadne");
        assert_eq!(
            zsh,
            "# >>> ariadne >>>\n\
             if [ -x \"/home/u/.local/bin/ariadne\" ] && (( $+functions[compdef] )); then\n    \
             source <(COMPLETE=zsh \"/home/u/.local/bin/ariadne\")\n\
             fi\n\
             # <<< ariadne <<<\n"
        );
        assert_eq!(
            block(InstallShell::Bash, "/opt/ariadne"),
            "# >>> ariadne >>>\n\
             [ -x \"/opt/ariadne\" ] && source <(COMPLETE=bash \"/opt/ariadne\")\n\
             # <<< ariadne <<<\n"
        );
    }

    /// Installing is idempotent: the block goes in once, and a second run
    /// leaves the file exactly as it was.
    #[test]
    fn installing_twice_writes_the_block_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rc = dir.path().join(".zshrc");
        std::fs::write(&rc, "").expect("empty rc");
        let block = block(InstallShell::Zsh, "/bin/ariadne");

        assert!(write_block(&rc, &block).expect("first"));
        let after = std::fs::read_to_string(&rc).expect("read");
        assert_eq!(after, block);

        assert!(!write_block(&rc, &block).expect("second"), "nothing to do");
        assert_eq!(std::fs::read_to_string(&rc).expect("read"), after);
    }

    /// An rc file that already has lines keeps them, and the block lands in
    /// place rather than a second time.
    #[test]
    fn an_existing_block_is_replaced_where_it_stands() {
        let rc = "export PATH=/x:$PATH\n\
                  # >>> ariadne >>>\n\
                  source <(COMPLETE=zsh \"/old/ariadne\")\n\
                  # <<< ariadne <<<\n\
                  alias l=ls\n";
        let block = block(InstallShell::Zsh, "/new/ariadne");
        let out = replace_block(rc, &block);
        assert_eq!(out, format!("export PATH=/x:$PATH\n{block}alias l=ls\n"));
        assert_eq!(out.matches(OPEN).count(), 1);
        assert_eq!(
            replace_block(&out, &block),
            out,
            "replacing it again changes nothing"
        );
    }

    /// A file that never ended in a newline does not get the marker glued to
    /// its last line.
    #[test]
    fn a_block_appended_to_an_unterminated_file_starts_on_its_own_line() {
        let block = block(InstallShell::Bash, "/bin/ariadne");
        assert_eq!(
            replace_block("alias l=ls", &block),
            format!("alias l=ls\n{block}")
        );
    }
}
