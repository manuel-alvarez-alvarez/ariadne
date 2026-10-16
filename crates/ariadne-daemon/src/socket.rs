//! Private unix-socket creation.

use std::io;
use std::path::Path;

use rustix::fs::Mode;
use rustix::process::umask;
use tokio::net::UnixListener;

/// Bind a Unix socket with owner-only permissions from its first instant.
pub fn bind_owner_only(path: &Path) -> io::Result<UnixListener> {
    let restore = RestoreUmask(umask(Mode::from_bits_retain(0o177)));
    let listener = UnixListener::bind(path);
    drop(restore);
    listener
}

struct RestoreUmask(Mode);

impl Drop for RestoreUmask {
    fn drop(&mut self) {
        umask(self.0);
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[tokio::test]
    async fn socket_is_owner_only_when_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ariadne.sock");
        let _listener = bind_owner_only(&path).unwrap();
        let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
