//! Confined snapshots of registered skill directories.
use ariadne_api::skills::SkillFile;
use std::path::{Component, Path};

pub(crate) fn snapshot(root: &Path) -> Result<Vec<SkillFile>, String> {
    fn walk(
        root: &Path,
        at: &Path,
        relative: &Path,
        stack: &mut Vec<std::path::PathBuf>,
        files: &mut Vec<SkillFile>,
    ) -> Result<(), String> {
        let resolved = at
            .canonicalize()
            .map_err(|e| format!("{}: {e}", at.display()))?;
        if !resolved.starts_with(root) {
            return Err(format!(
                "symbolic link escapes skill directory: {}",
                at.display()
            ));
        }
        if stack.contains(&resolved) {
            return Err(format!("symbolic link cycle: {}", at.display()));
        }
        let meta = std::fs::metadata(&resolved).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            stack.push(resolved.clone());
            let mut entries = std::fs::read_dir(&resolved)
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                walk(
                    root,
                    &at.join(entry.file_name()),
                    &relative.join(entry.file_name()),
                    stack,
                    files,
                )?;
            }
            stack.pop();
        } else if meta.is_file() && relative != Path::new("SKILL.md") {
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                meta.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            files.push(SkillFile {
                path: relative.to_string_lossy().replace('\\', "/"),
                bytes: std::fs::read(&resolved).map_err(|e| e.to_string())?,
                executable,
            });
        } else if !meta.is_file() {
            return Err(format!("unsupported skill entry: {}", at.display()));
        }
        Ok(())
    }
    let metadata = std::fs::symlink_metadata(root).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("symbolic link at skill root: {}", root.display()));
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    walk(&root, &root, Path::new(""), &mut Vec::new(), &mut files)?;
    Ok(files)
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn snapshot_rejects_symlink_at_skill_root() {
        let dir = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        std::fs::write(external.path().join("secret"), b"private").unwrap();
        let root = dir.path().join("skill");
        std::os::unix::fs::symlink(external.path(), &root).unwrap();

        assert!(
            super::snapshot(&root)
                .unwrap_err()
                .contains("symbolic link")
        );
    }
}

pub(crate) fn validate(files: &[SkillFile]) -> Result<(), String> {
    let mut paths = std::collections::HashSet::new();
    for file in files {
        let path = Path::new(&file.path);
        if file.path.is_empty()
            || file.path.contains('\\')
            || file.path.contains(':')
            || path == Path::new("SKILL.md")
            || !path.components().all(|c| matches!(c, Component::Normal(_)))
            || !paths.insert(path)
        {
            return Err(format!("invalid bundle path: {}", file.path));
        }
    }
    Ok(())
}

/// Build privately before the caller changes the registered document.
pub(crate) fn stage(
    parent: &Path,
    document: &str,
    files: &[SkillFile],
) -> Result<tempfile::TempDir, String> {
    validate(files)?;
    let dir = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
    std::fs::write(dir.path().join("SKILL.md"), document).map_err(|e| e.to_string())?;
    for file in files {
        let path = dir.path().join(&file.path);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&path, &file.bytes).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                &path,
                std::fs::Permissions::from_mode(if file.executable { 0o755 } else { 0o644 }),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(dir)
}

/// Explicit Markdown links are warnings; prose and shell commands are not inspected.
pub(crate) fn warnings(root: &Path, document: &str, files: &[SkillFile]) -> Vec<String> {
    fn links(root: &Path, parent: &Path, document: &str, warnings: &mut Vec<String>) {
        for event in pulldown_cmark::Parser::new(document) {
            let dest_url = match event {
                pulldown_cmark::Event::Start(
                    pulldown_cmark::Tag::Link { dest_url, .. }
                    | pulldown_cmark::Tag::Image { dest_url, .. },
                ) => dest_url,
                _ => continue,
            };
            let target = dest_url.split(['#', '?']).next().unwrap_or_default();
            if target.is_empty()
                || target.starts_with("//")
                || target.contains("://") && !target.starts_with("file://")
                || target.starts_with("mailto:")
            {
                continue;
            }
            let target = target.strip_prefix("file://").unwrap_or(target);
            let path = parent.join(target);
            let mut normalized = std::path::PathBuf::new();
            for part in path.components() {
                match part {
                    Component::ParentDir => {
                        normalized.pop();
                    }
                    Component::CurDir => {}
                    other => normalized.push(other.as_os_str()),
                }
            }
            if !normalized.starts_with(root) {
                warnings.push(format!("external Markdown file link: {dest_url}"));
            }
        }
    }
    let mut warnings = Vec::new();
    links(root, root, document, &mut warnings);
    for file in files.iter().filter(|f| f.path.ends_with(".md")) {
        if let Ok(document) = std::str::from_utf8(&file.bytes) {
            links(
                root,
                root.join(&file.path).parent().unwrap(),
                document,
                &mut warnings,
            );
        }
    }
    warnings.sort();
    warnings.dedup();
    warnings
}
