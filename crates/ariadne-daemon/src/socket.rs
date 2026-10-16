//! Private unix-socket creation.

use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;

use tokio::net::UnixListener;

/// Bind a Unix socket with owner-only permissions from its first instant.
pub fn bind_owner_only(path: &Path) -> io::Result<UnixListener> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let directory = private_directory(parent)?;
    let temporary = directory.join("s");

    let listener = match UnixListener::bind(&temporary) {
        Ok(listener) => listener,
        Err(error) => {
            let _ = fs::remove_dir(&directory);
            return Err(error);
        }
    };
    if let Err(error) = fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)) {
        remove_temporary_socket(&temporary, &directory);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, path) {
        remove_temporary_socket(&temporary, &directory);
        return Err(error);
    }
    if let Err(error) = fs::remove_dir(&directory) {
        let _ = fs::remove_file(path);
        return Err(error);
    }

    Ok(listener)
}

fn private_directory(parent: &Path) -> io::Result<std::path::PathBuf> {
    for _ in 0..10 {
        let mut random = [0; 8];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let directory = parent.join(format!(".a-{}", hex::encode(random)));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a private socket directory",
    ))
}

fn remove_temporary_socket(socket: &Path, directory: &Path) {
    let _ = fs::remove_file(socket);
    let _ = fs::remove_dir(directory);
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
