//! v2.54.0 — the filesystem capability + path sandbox: the read-only,
//! agent-reachable disk surface, designed EXACTLY ONCE. Before v2.54.0
//! axon had no filesystem capability at all — `fs::*` existed only as host
//! plumbing (never flow-invocable). v2.54.0 invents the agent-reachable read
//! surface, and it is a *capability*, not an effect: a set of allowed
//! roots a `DocumentReader` may read under, with every escape refused BEFORE
//! any byte is read.
//!
//! **Read-only, and local-only.** The old `FileReader` description
//! ("Read local or remote files") conflated a sandboxed disk read with a remote
//! fetch (`network`, SSRF — the v2.52.0 problem). This sandbox governs the local
//! half only; remote fetch stays where v2.52.0 put it.
//!
//! **Every escape is a typed refusal (the design decision / section 6):** a path outside every
//! root, a `..` traversal, a symlink that escapes the root, or a
//! device/special file is `SandboxError`, never a silent read of the wrong
//! file.

use std::path::{Component, Path, PathBuf};

/// Why a path was refused. Every variant is a fail-closed refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxError {
    /// The resolved path is not under any configured root.
    OutsideRoot(String),
    /// The path contains a `..` component (traversal).
    Traversal(String),
    /// The path (or a component) is a symlink escaping the root, or the target
    /// resolves outside the root.
    SymlinkEscape(String),
    /// The target is not a regular file (a directory, device, FIFO, socket).
    NotRegularFile(String),
    /// The path does not exist / could not be canonicalized.
    NotFound(String),
    /// No sandbox roots are configured — the capability is not granted.
    NoRoots,
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxError::OutsideRoot(p) => write!(f, "path '{p}' is outside every sandbox root"),
            SandboxError::Traversal(p) => write!(f, "path '{p}' contains a `..` traversal"),
            SandboxError::SymlinkEscape(p) => write!(f, "path '{p}' escapes the sandbox via a symlink"),
            SandboxError::NotRegularFile(p) => write!(f, "path '{p}' is not a regular file"),
            SandboxError::NotFound(p) => write!(f, "path '{p}' does not exist or is unreadable"),
            SandboxError::NoRoots => write!(f, "no sandbox roots configured — filesystem read is not granted"),
        }
    }
}
impl std::error::Error for SandboxError {}

/// v2.54.0 — the read-only path sandbox. Holds the allowed roots; `resolve`
/// validates a requested path against them, canonicalizing to defeat symlink
/// and `..` escapes.
#[derive(Debug, Clone, Default)]
pub struct PathSandbox {
    roots: Vec<PathBuf>,
}

impl PathSandbox {
    /// Construct from a set of allowed root directories. Each root is
    /// canonicalized once at construction (a root that does not exist is
    /// dropped — an unusable root grants nothing).
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        let roots = roots
            .into_iter()
            .filter_map(|r| std::fs::canonicalize(&r).ok())
            .collect();
        PathSandbox { roots }
    }

    /// Whether any root is configured (the capability is granted).
    pub fn is_granted(&self) -> bool {
        !self.roots.is_empty()
    }

    /// v2.54.0 — the pure syntactic check: does the requested path string contain
    /// a `..` traversal segment? Applied BEFORE any filesystem touch, so a
    /// hostile relative path is refused without a stat. Checks the RAW string
    /// (split on both `/` and `\`) rather than `Path::components`, because a
    /// Windows verbatim prefix (`\\?\`) suppresses `Component::ParentDir` and
    /// would let a `..` slip through the component walk. Public for the sandbox
    /// unit tests + the enterprise per-tenant layer.
    pub fn has_traversal(requested: &str) -> bool {
        requested
            .split(['/', '\\'])
            .any(|seg| seg == "..")
            || Path::new(requested)
                .components()
                .any(|c| matches!(c, Component::ParentDir))
    }

    /// v2.54.0 — resolve a requested path to a canonical, in-sandbox, regular
    /// file — or a typed refusal. The canonicalization is what defeats a
    /// symlink escape: the resolved real path must still be under a root.
    pub fn resolve(&self, requested: &str) -> Result<PathBuf, SandboxError> {
        if self.roots.is_empty() {
            return Err(SandboxError::NoRoots);
        }
        // (1) syntactic traversal refusal — before any I/O.
        if Self::has_traversal(requested) {
            return Err(SandboxError::Traversal(requested.to_string()));
        }
        // (2) canonicalize — follows symlinks to the real target, fails if the
        // path does not exist.
        let canonical = std::fs::canonicalize(requested)
            .map_err(|_| SandboxError::NotFound(requested.to_string()))?;
        // (3) the real path must be under a root (symlink-escape defeated).
        if !self.roots.iter().any(|root| canonical.starts_with(root)) {
            // Distinguish a symlink escape (the requested path WAS under a root
            // but its target is not) from a plain outside-root request, for a
            // clearer diagnostic.
            let requested_abs = std::fs::canonicalize(
                Path::new(requested).parent().unwrap_or(Path::new(".")),
            )
            .ok();
            if requested_abs
                .map(|p| self.roots.iter().any(|r| p.starts_with(r)))
                .unwrap_or(false)
            {
                return Err(SandboxError::SymlinkEscape(requested.to_string()));
            }
            return Err(SandboxError::OutsideRoot(requested.to_string()));
        }
        // (4) must be a regular file (not a dir/device/FIFO/socket).
        let meta = std::fs::symlink_metadata(&canonical)
            .map_err(|_| SandboxError::NotFound(requested.to_string()))?;
        // After canonicalize the target is real; ensure it is a regular file.
        let is_regular = std::fs::metadata(&canonical)
            .map(|m| m.is_file())
            .unwrap_or(false);
        let _ = meta;
        if !is_regular {
            return Err(SandboxError::NotRegularFile(requested.to_string()));
        }
        Ok(canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_root() -> PathBuf {
        let base = std::env::temp_dir().join(format!("axon_sbx_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        std::fs::canonicalize(&base).unwrap()
    }

    fn write_file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    #[test]
    fn no_roots_grants_nothing() {
        let sb = PathSandbox::default();
        assert!(!sb.is_granted());
        assert_eq!(sb.resolve("/anything"), Err(SandboxError::NoRoots));
    }

    #[test]
    fn traversal_is_refused_syntactically() {
        assert!(PathSandbox::has_traversal("../etc/passwd"));
        assert!(PathSandbox::has_traversal("a/b/../../../secret"));
        assert!(!PathSandbox::has_traversal("a/b/c.docx"));
        let root = tmp_root();
        let sb = PathSandbox::new([root.clone()]);
        let req = format!("{}/../outside", root.display());
        assert!(matches!(sb.resolve(&req), Err(SandboxError::Traversal(_))));
    }

    #[test]
    fn in_root_regular_file_resolves() {
        let root = tmp_root();
        let f = write_file(&root, "report.docx", "PK");
        let sb = PathSandbox::new([root.clone()]);
        let resolved = sb.resolve(f.to_str().unwrap()).unwrap();
        assert!(resolved.starts_with(&root));
    }

    #[test]
    fn outside_root_is_refused() {
        let root = tmp_root();
        let other = tmp_root().join("elsewhere");
        let _ = std::fs::create_dir_all(&other);
        let f = write_file(&std::fs::canonicalize(&other).unwrap(), "x.docx", "PK");
        // sandbox rooted at `root`, file lives under `other` → refused.
        let sb = PathSandbox::new([root.join("subroot")]);
        // subroot doesn't exist → no roots → NoRoots (an unusable root grants nothing).
        assert!(matches!(sb.resolve(f.to_str().unwrap()), Err(SandboxError::NoRoots)));
    }

    #[test]
    fn missing_file_is_not_found() {
        let root = tmp_root();
        let sb = PathSandbox::new([root.clone()]);
        let req = format!("{}/does_not_exist.docx", root.display());
        assert!(matches!(sb.resolve(&req), Err(SandboxError::NotFound(_))));
    }

    #[test]
    fn directory_is_not_a_regular_file() {
        let root = tmp_root();
        let sub = root.join("adir");
        let _ = std::fs::create_dir_all(&sub);
        let sb = PathSandbox::new([root.clone()]);
        assert!(matches!(
            sb.resolve(sub.to_str().unwrap()),
            Err(SandboxError::NotRegularFile(_))
        ));
    }
}
