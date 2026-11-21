//! Provenance of this daemon, captured when Cargo builds it.
use ariadne_api::hosts::{BinaryMethod, BinarySource, InspectedBinaries};

pub(crate) struct Build {
    pub(crate) repository: Option<&'static str>,
    pub(crate) checkout: &'static str,
    pub(crate) commit: &'static str,
    pub(crate) uncommitted: bool,
}

pub(crate) const BUILD: Build = Build {
    repository: if env!("ARIADNE_RELEASE_REPOSITORY").is_empty() {
        None
    } else {
        Some(env!("ARIADNE_RELEASE_REPOSITORY"))
    },
    checkout: env!("ARIADNE_BUILD_CHECKOUT"),
    commit: env!("ARIADNE_BUILD_COMMIT"),
    uncommitted: matches!(env!("ARIADNE_BUILD_UNCOMMITTED").as_bytes(), b"true"),
};

impl Build {
    pub(crate) fn binaries(&self) -> InspectedBinaries {
        InspectedBinaries {
            method: if self.repository.is_some() {
                BinaryMethod::Release
            } else {
                BinaryMethod::SourceBuild
            },
            version: env!("CARGO_PKG_VERSION").into(),
            tag: self
                .repository
                .map(|_| format!("v{}", env!("CARGO_PKG_VERSION"))),
            commit: Some(self.commit.into()),
            uncommitted: self.uncommitted,
        }
    }

    pub(crate) fn source(&self) -> Result<BinarySource, String> {
        match self.repository {
            Some(repository) => Ok(BinarySource::Release {
                repository: repository.into(),
                tag: format!("v{}", env!("CARGO_PKG_VERSION")),
            }),
            None => Err(format!(
                "This daemon is a source build from {}. Select an explicit local or release source.",
                self.checkout
            )),
        }
    }
}
