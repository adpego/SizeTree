//! Walks a directory tree once, in parallel, and builds an in-memory size tree.

use std::collections::HashSet;
use std::fs::{self, DirEntry, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use rayon::prelude::*;

pub struct ScanOptions {
    /// Report file lengths instead of allocated disk space.
    pub apparent_size: bool,
    /// Skip directories that live on a different device than the root.
    pub one_file_system: bool,
    /// Children are kept only up to this depth; deeper sizes are still counted.
    pub max_depth: Option<usize>,
    /// Entries whose file name matches any of these are ignored.
    pub exclude: Vec<glob::Pattern>,
}

#[derive(Debug)]
pub struct Node {
    pub name: String,
    pub size: u64,
    pub kind: Kind,
    pub children: Vec<Node>,
}

#[derive(Debug)]
pub enum Kind {
    Dir,
    File,
    Symlink { target: Option<String> },
    Other,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Dir => "directory",
            Kind::File => "file",
            Kind::Symlink { .. } => "symlink",
            Kind::Other => "other",
        }
    }
}

#[derive(Debug)]
pub struct ScanError {
    pub path: PathBuf,
    pub error: io::Error,
}

/// Live counters, readable while a scan is running (e.g. for progress output).
#[derive(Default)]
pub struct Stats {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
}

pub struct ScanResult {
    pub root: Node,
    pub errors: Vec<ScanError>,
}

/// Scans `path`. Only failing to access `path` itself is an error; problems
/// further down are collected in [`ScanResult::errors`].
pub fn scan(path: &Path, opts: &ScanOptions, stats: &Stats) -> io::Result<ScanResult> {
    // The root is the one place where symlinks are followed: the user asked for it.
    let meta = fs::metadata(path)?;
    let scanner = Scanner {
        opts,
        stats,
        root_dev: device(&meta),
        seen_inodes: Mutex::default(),
        errors: Mutex::default(),
    };
    let root = scanner.node(path, path.display().to_string(), &meta, 0);

    let mut errors = scanner
        .errors
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    errors.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ScanResult { root, errors })
}

struct Scanner<'a> {
    opts: &'a ScanOptions,
    stats: &'a Stats,
    root_dev: Option<u64>,
    seen_inodes: Mutex<HashSet<(u64, u64)>>,
    errors: Mutex<Vec<ScanError>>,
}

impl Scanner<'_> {
    fn node(&self, path: &Path, name: String, meta: &Metadata, depth: usize) -> Node {
        let own_size = self.size_of(meta);

        if meta.is_dir() {
            // The root itself is not counted: stats describe what is inside it.
            if depth > 0 {
                self.stats.dirs.fetch_add(1, Ordering::Relaxed);
            }
            let mut children = self.children(path, depth + 1);
            let size = own_size + children.iter().map(|c| c.size).sum::<u64>();
            if self.opts.max_depth.is_some_and(|max| depth >= max) {
                children = Vec::new();
            }
            return Node {
                name,
                size,
                kind: Kind::Dir,
                children,
            };
        }

        self.stats.files.fetch_add(1, Ordering::Relaxed);
        let kind = if meta.is_symlink() {
            let target = fs::read_link(path).ok();
            Kind::Symlink {
                target: target.map(|t| t.display().to_string()),
            }
        } else if meta.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        Node {
            name,
            size: own_size,
            kind,
            children: Vec::new(),
        }
    }

    fn children(&self, dir: &Path, depth: usize) -> Vec<Node> {
        let entries: Vec<DirEntry> = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|entry| entry.map_err(|e| self.error(dir, e)).ok())
                .collect(),
            Err(e) => {
                self.error(dir, e);
                return Vec::new();
            }
        };

        entries
            .into_par_iter()
            .filter_map(|entry| self.child(&entry, depth))
            .collect()
    }

    fn child(&self, entry: &DirEntry, depth: usize) -> Option<Node> {
        let name = entry.file_name().to_string_lossy().into_owned();
        if self.opts.exclude.iter().any(|p| p.matches(&name)) {
            return None;
        }

        let path = entry.path();
        // Does not follow symlinks, so links are never traversed or double counted.
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(e) => {
                self.error(&path, e);
                return None;
            }
        };

        if self.opts.one_file_system && meta.is_dir() && device(&meta) != self.root_dev {
            return None;
        }
        // Like `du`, a file with several hard links is only counted (and shown) once.
        if let Some(key) = hard_link_key(&meta)
            && !self
                .seen_inodes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key)
        {
            return None;
        }

        Some(self.node(&path, name, &meta, depth))
    }

    fn size_of(&self, meta: &Metadata) -> u64 {
        if self.opts.apparent_size {
            // A directory's length is file-system specific bookkeeping (4096 on
            // ext4, name lengths on btrfs…), so like `du` only file data counts.
            if meta.is_dir() { 0 } else { meta.len() }
        } else {
            disk_usage(meta)
        }
    }

    fn error(&self, path: &Path, error: io::Error) {
        let error = ScanError {
            path: path.to_path_buf(),
            error,
        };
        self.errors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(error);
    }
}

#[cfg(unix)]
fn disk_usage(meta: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    // st_blocks is always in 512-byte units, whatever the file system block size.
    meta.blocks() * 512
}

#[cfg(not(unix))]
fn disk_usage(meta: &Metadata) -> u64 {
    meta.len()
}

#[cfg(unix)]
fn device(meta: &Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.dev())
}

#[cfg(not(unix))]
fn device(_meta: &Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn hard_link_key(meta: &Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    (!meta.is_dir() && meta.nlink() > 1).then(|| (meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn hard_link_key(_meta: &Metadata) -> Option<(u64, u64)> {
    None
}
