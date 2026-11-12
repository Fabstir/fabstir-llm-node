// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The renderer's input and output folders (D9). ComfyUI has no delete call, so the node removes files through the
//! mounted folders: a refused job's files at once, and every job's files at job end (question 3, legal 2.4). Names
//! come from the renderer's own answers and are still treated as untrusted: nothing outside the two roots is ever
//! touched, and the evidence directory is never a root.

use super::config::RendererDirs;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// What one job put on the renderer's disk, recorded as the renderer names each file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct JobFiles {
    /// Files under the input folder (`name`, or `sub/name`).
    pub inputs: Vec<String>,
    /// Whole input subfolders (a deep-conform frame set).
    pub input_dirs: Vec<String>,
    /// `(subfolder, filename)` under the output folder.
    pub outputs: Vec<(String, String)>,
}

impl JobFiles {
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.input_dirs.is_empty() && self.outputs.is_empty()
    }
}

/// Join an untrusted relative name onto `root`, refusing anything that could leave it.
pub fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.contains('\0') {
        return None;
    }
    let rel = Path::new(rel);
    let mut out = root.to_path_buf();
    let mut depth = 0;
    for c in rel.components() {
        match c {
            Component::Normal(p) => {
                out.push(p);
                depth += 1;
            }
            Component::CurDir => {}
            _ => return None, // absolute, parent or prefix components
        }
    }
    (depth > 0).then_some(out)
}

/// Where an untrusted relative name really points.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    /// A path whose parent is, after resolving symlinks, still inside the root.
    Inside(PathBuf),
    /// The parent does not exist: there is nothing to delete.
    Missing,
    /// The name leaves the root, lexically or through a symlinked parent. Never touched.
    Outside,
}

/// Resolve `rel` under `root` for deletion (fix F10): [`safe_join`], then the PARENT is canonicalised and must
/// still lie under the canonical root, so a symlink planted in a folder the renderer can write to cannot steer a
/// delete elsewhere (into the evidence directory, say). The last component is not followed: removing a symlink
/// removes the link.
pub fn resolve_under(root: &Path, rel: &str) -> Target {
    let Some(joined) = safe_join(root, rel) else {
        return Target::Outside;
    };
    let (Some(parent), Some(name)) = (joined.parent(), joined.file_name()) else {
        return Target::Outside;
    };
    let Ok(root) = root.canonicalize() else {
        return Target::Outside;
    };
    match parent.canonicalize() {
        Ok(real) if real.starts_with(&root) => Target::Inside(real.join(name)),
        Ok(_) => Target::Outside,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Target::Missing,
        Err(_) => Target::Outside,
    }
}

/// In-process use counts for input names. Inputs are content-addressed, so two jobs running at once (more than one
/// generation slot) can share a file; it is deleted only when the last of them ends.
#[derive(Default)]
pub struct InUse(Mutex<HashMap<String, usize>>);

impl InUse {
    pub fn hold(&self, key: &str) {
        if let Ok(mut g) = self.0.lock() {
            *g.entry(key.to_string()).or_insert(0) += 1;
        }
    }
    /// Release one hold; `true` when nobody holds the name any more (or the count is unknown).
    pub fn release(&self, key: &str) -> bool {
        let Ok(mut g) = self.0.lock() else {
            return false; // unknown: leave the file rather than break a running job
        };
        match g.get_mut(key) {
            Some(n) if *n > 1 => {
                *n -= 1;
                false
            }
            _ => {
                g.remove(key);
                true
            }
        }
    }
}

/// Delete a job's files. Blocking; returns `(removed, failed)`. A missing file counts as removed.
pub fn delete_job_files(dirs: &RendererDirs, files: &JobFiles, in_use: &InUse) -> (usize, usize) {
    let (mut removed, mut failed) = (0usize, 0usize);
    let mut remove = |root: &Path, rel: &str, dir: bool| match resolve_under(root, rel) {
        Target::Inside(p) => {
            let done = if dir {
                std::fs::remove_dir_all(&p)
            } else {
                std::fs::remove_file(&p)
            };
            match done {
                Ok(()) => removed += 1,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => removed += 1,
                Err(_) => failed += 1,
            }
        }
        Target::Missing => removed += 1,
        Target::Outside => failed += 1,
    };
    for name in &files.inputs {
        if in_use.release(&format!("f:{name}")) {
            remove(&dirs.input, name, false);
        }
    }
    for sub in &files.input_dirs {
        if in_use.release(&format!("d:{sub}")) {
            remove(&dirs.input, sub, true);
        }
    }
    for (sub, name) in &files.outputs {
        let rel = if sub.is_empty() {
            name.clone()
        } else {
            format!("{sub}/{name}")
        };
        remove(&dirs.output, &rel, false);
    }
    (removed, failed)
}

/// [`delete_job_files`] off the async workers and bounded: job-end deletion runs while the generation slot is still
/// held (so the next job cannot be handed a file this one is deleting), and must never hold it for long.
pub async fn delete_job_files_bounded(
    dirs: RendererDirs,
    files: JobFiles,
    in_use: std::sync::Arc<InUse>,
    limit: Duration,
) -> Result<(usize, usize), String> {
    let task = tokio::task::spawn_blocking(move || delete_job_files(&dirs, &files, &in_use));
    match tokio::time::timeout(limit, task).await {
        Ok(Ok(counts)) => Ok(counts),
        Ok(Err(e)) => Err(format!("delete task failed: {e}")),
        Err(_) => Err("delete timed out".to_string()),
    }
}
