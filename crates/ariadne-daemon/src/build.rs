//! Provenance of this daemon, captured when Cargo builds it.
use ariadne_api::hosts::{BinaryMethod, InspectedBinaries};

/// What Cargo recorded about this daemon's build. The test harness replaces
/// it through [`crate::config::Config::build`]: it is not a key of `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// The repository whose release workflow built this daemon, or `None` for
    /// a source build.
    pub repository: Option<String>,
    /// The checkout a source build came from, on this machine.
    pub checkout: String,
    pub commit: String,
    /// Whether the checkout had uncommitted edits when Cargo built it.
    pub uncommitted: bool,
}

impl Build {
    /// The record Cargo wrote into this binary.
    pub fn recorded() -> Self {
        Build {
            repository: Some(env!("ARIADNE_RELEASE_REPOSITORY"))
                .filter(|r| !r.is_empty())
                .map(String::from),
            checkout: env!("ARIADNE_BUILD_CHECKOUT").into(),
            commit: env!("ARIADNE_BUILD_COMMIT").into(),
            uncommitted: matches!(env!("ARIADNE_BUILD_UNCOMMITTED").as_bytes(), b"true"),
        }
    }

    pub(crate) fn binaries(&self) -> InspectedBinaries {
        InspectedBinaries {
            method: if self.repository.is_some() {
                BinaryMethod::Release
            } else {
                BinaryMethod::SourceBuild
            },
            version: env!("CARGO_PKG_VERSION").into(),
            tag: self.tag(),
            commit: Some(self.commit.clone()),
            uncommitted: self.uncommitted,
        }
    }

    /// The release tag of this daemon's version, for a release build.
    pub(crate) fn tag(&self) -> Option<String> {
        self.repository
            .as_ref()
            .map(|_| format!("v{}", env!("CARGO_PKG_VERSION")))
    }

    /// Check that this machine still has the checkout and the commit that a
    /// source build packs. The reason names the missing thing.
    pub(crate) async fn check_source(&self) -> Result<(), String> {
        let checkout = std::path::Path::new(&self.checkout);
        let tree = tokio::process::Command::new("git")
            .arg("-C")
            .arg(checkout)
            .args(["rev-parse", "--git-dir"])
            .output()
            .await;
        if !checkout.is_dir() || !tree.is_ok_and(|o| o.status.success()) {
            return Err(format!(
                "The Ariadne checkout {} that built this daemon is missing on this machine. Restore the checkout, or select an explicit local or release source.",
                self.checkout
            ));
        }
        let commit = tokio::process::Command::new("git")
            .arg("-C")
            .arg(checkout)
            .args(["cat-file", "-e"])
            .arg(format!("{}^{{commit}}", self.commit))
            .output()
            .await;
        if !commit.is_ok_and(|o| o.status.success()) {
            return Err(format!(
                "The commit {} that built this daemon is missing from the checkout {}. Fetch the commit into the checkout, or select an explicit local or release source.",
                self.commit, self.checkout
            ));
        }
        Ok(())
    }
}
