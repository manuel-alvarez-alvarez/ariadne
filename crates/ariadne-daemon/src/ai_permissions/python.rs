//! Which Python the model install would run on, and whether it is new enough.
//!
//! The model needs Python 3.12 or 3.13, and installs PyTorch into a virtual
//! environment of that interpreter. The question is asked before anything is
//! downloaded, because a 3.9 finds out two gigabytes too late.

use std::ffi::OsStr;
use std::path::PathBuf;

use ariadne_api::permissions::PythonDto;
use ariadne_core::probe;

/// The Python versions the model runs on.
const SUPPORTED: [(u32, u32); 2] = [(3, 12), (3, 13)];

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
        None => ["python3.13", "python3.12", "python3"]
            .into_iter()
            .find_map(|name| path.and_then(|path| probe::which(path, name))),
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
        ok: version.as_deref().is_some_and(supported),
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

/// Whether a version is Python 3.12 or 3.13. Anything that does not read as two
/// numbers is not: an interpreter that will not say what it is, is not one to
/// install two gigabytes into.
pub(crate) fn supported(version: &str) -> bool {
    let mut parts = version.split('.').map(str::parse::<u32>);
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => SUPPORTED.contains(&(major, minor)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What an interpreter prints is read as a version, whichever of the two
    /// shapes it uses, and the supported versions are 3.12 and 3.13.
    #[test]
    fn the_version_is_read_from_the_line_and_checked_against_kevs_versions() {
        assert_eq!(version_of("Python 3.12.1"), "3.12.1");
        assert_eq!(version_of("3.9.18"), "3.9.18");
        assert_eq!(version_of("Python 3.10.0rc1"), "3.10.0rc1");
        // Nothing that reads as a version: the line itself is the answer.
        assert_eq!(version_of("not a python at all"), "not a python at all");

        assert!(supported("3.12.0"));
        assert!(supported("3.13.1"));
        assert!(!supported("3.14.0"));
        assert!(!supported("3.11.18"));
        assert!(!supported("4.0.0"));
        // A version with no minor, and a line that is no version.
        assert!(!supported("3"));
        assert!(!supported("not a python at all"));
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
