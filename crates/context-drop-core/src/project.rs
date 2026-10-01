//! Project detection for routing.
//!
//! The project root is the git top-level (`git rev-parse --show-toplevel`) when
//! inside a repository, otherwise the canonicalized cwd. Both the cwd and the
//! project root are recorded. A subdirectory of a monorepo is NOT treated as a
//! separate project unless the git root actually differs.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Detected project identity for a working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    /// Canonicalized current working directory.
    pub cwd: String,
    /// Git top-level, or the canonicalized cwd when not in a repository.
    pub project_root: String,
    /// Basename of the project root.
    pub project_name: String,
    /// Whether the cwd is inside a git repository.
    pub is_git: bool,
}

/// Detect project info for `cwd` using the real `git` binary.
pub fn detect(cwd: &Path) -> ProjectInfo {
    detect_with(cwd, git_toplevel)
}

/// Detect project info using an injectable git-toplevel resolver (for tests).
pub fn detect_with<F>(cwd: &Path, git_toplevel: F) -> ProjectInfo
where
    F: Fn(&Path) -> Option<PathBuf>,
{
    let canonical_cwd = canonicalize(cwd);
    match git_toplevel(&canonical_cwd) {
        Some(root) => {
            let root = canonicalize(&root);
            ProjectInfo {
                cwd: path_to_string(&canonical_cwd),
                project_name: basename(&root),
                project_root: path_to_string(&root),
                is_git: true,
            }
        }
        None => ProjectInfo {
            project_name: basename(&canonical_cwd),
            project_root: path_to_string(&canonical_cwd),
            cwd: path_to_string(&canonical_cwd),
            is_git: false,
        },
    }
}

/// Run `git -C <dir> rev-parse --show-toplevel`.
fn git_toplevel(dir: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

fn canonicalize(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn path_to_string(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

fn basename(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path_to_string(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_git_uses_cwd_as_root() {
        let dir = tempfile::tempdir().unwrap();
        let info = detect_with(dir.path(), |_| None);
        assert!(!info.is_git);
        assert_eq!(info.project_root, info.cwd);
        assert_eq!(info.project_name, basename(&canonicalize(dir.path())));
    }

    #[test]
    fn git_uses_toplevel_as_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let sub = root.join("packages").join("app");
        std::fs::create_dir_all(&sub).unwrap();
        // Simulate a monorepo: any cwd resolves to the same top-level.
        let info = detect_with(&sub, |_| Some(root.clone()));
        assert!(info.is_git);
        assert_eq!(info.project_root, path_to_string(&canonicalize(&root)));
        // The subdirectory is NOT a separate project.
        assert_ne!(info.cwd, info.project_root);
        assert_eq!(info.project_name, basename(&canonicalize(&root)));
    }

    #[test]
    fn real_git_repo_is_detected() {
        // Uses the real git binary; skip cleanly if git is unavailable.
        let dir = tempfile::tempdir().unwrap();
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .arg("init")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("git not available; skipping real_git_repo_is_detected");
            return;
        }
        let info = detect(dir.path());
        assert!(info.is_git);
        assert_eq!(
            canonicalize(Path::new(&info.project_root)),
            canonicalize(dir.path())
        );
    }
}
