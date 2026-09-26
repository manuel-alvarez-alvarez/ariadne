//! Which Python the Laya install would run on, and whether it is new enough.
//!
//! Laya needs Python 3.10 or newer, and installs PyTorch into a virtual
//! environment of that interpreter. The question is asked before anything is
//! downloaded, because a 3.9 finds out two gigabytes too late.

use std::ffi::OsStr;
use std::path::PathBuf;

use ariadne_api::permissions::PythonDto;
use ariadne_core::probe;

/// The oldest Python Laya runs on.
const OLDEST: (u32, u32) = (3, 10);

/// The interpreter the daemon would install into, as it answers `--version`.
///
/// `configured` is the `python_bin` config key: a path is taken as it stands
/// and a bare name is looked up on `path`, the daemon's own `PATH`, which is
/// also where `python3` is looked up when nothing is configured. A probe that
/// finds nothing, or a version nothing can be read out of, answers `ok:
/// false` rather than failing: the caller's next move is to say so, never to
/// stop.
pub(crate) async fn probe_python(configured: Option<&str>, path: Option<&OsStr>) -> PythonDto {
    let found = match configured {
        Some(bin) if bin.contains('/') => {
            let candidate = PathBuf::from(bin);
            probe::is_executable(&candidate).then_some(candidate)
        }
        Some(bin) => path.and_then(|path| probe::which(path, bin)),
        None => path.and_then(|path| probe::which(path, "python3")),
    };
    let Some(binary) = found else {
        return PythonDto {
            path: None,
            version: None,
            ok: false,
        };
    };
    let version = probe::probe_version(&binary, "--version")
        .await
        .map(|line| version_of(&line));
    PythonDto {
        path: Some(binary.display().to_string()),
        ok: version.as_deref().is_some_and(new_enough),
        version,
    }
}

/// The version out of what the interpreter printed: `Python 3.12.1` is
/// `3.12.1`. A line that names no version is kept as it stands, so a reader
/// sees what the binary actually said.
fn version_of(line: &str) -> String {
    line.split_whitespace()
        .find(|word| word.starts_with(|c: char| c.is_ascii_digit()))
        .unwrap_or(line)
        .to_string()
}

/// Whether a version is 3.10 or newer. Anything that does not read as two
/// numbers is not: an interpreter that will not say what it is, is not one to
/// install two gigabytes into.
fn new_enough(version: &str) -> bool {
    let mut parts = version.split('.').map(str::parse::<u32>);
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => (major, minor) >= OLDEST,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What an interpreter prints is read as a version, whichever of the two
    /// shapes it uses, and the cut is at 3.10 exactly.
    #[test]
    fn the_version_is_read_from_the_line_and_cut_at_three_ten() {
        assert_eq!(version_of("Python 3.12.1"), "3.12.1");
        assert_eq!(version_of("3.9.18"), "3.9.18");
        assert_eq!(version_of("Python 3.10.0rc1"), "3.10.0rc1");
        // Nothing that reads as a version: the line itself is the answer.
        assert_eq!(version_of("not a python at all"), "not a python at all");

        assert!(new_enough("3.10.0"));
        assert!(new_enough("3.12.1"));
        assert!(new_enough("4.0.0"));
        assert!(!new_enough("3.9.18"));
        assert!(!new_enough("2.7.18"));
        // A version with no minor, and a line that is no version.
        assert!(!new_enough("3"));
        assert!(!new_enough("not a python at all"));
    }

    /// A `python_bin` naming nothing that can be run leaves the report empty
    /// rather than pointing at a file the install would fail on.
    #[tokio::test]
    async fn an_interpreter_that_is_not_there_is_reported_as_missing() {
        let report = probe_python(Some("/nonexistent/python3"), None).await;
        assert_eq!(report.path, None);
        assert_eq!(report.version, None);
        assert!(!report.ok);
    }
}
