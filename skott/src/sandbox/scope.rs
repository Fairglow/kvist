use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::{
    Error, Preflight, Result, admit_scope_entry, io_error, queue_scope_directory,
    resolve_link_target, target_escapes_scope,
};

const WORKERS: usize = 8;
const IDLE_POLL: Duration = Duration::from_millis(100);

pub(super) fn check_writable_scope(workdir: &Path, preflight: &Preflight<'_>) -> Result<()> {
    check_writable_scope_with(workdir, preflight, &|_| Ok(()))
}

pub(super) fn check_writable_scope_with(
    workdir: &Path,
    preflight: &Preflight<'_>,
    visit: &(impl Fn(&Path) -> Result<()> + Sync),
) -> Result<()> {
    preflight.check()?;
    let started = Instant::now();
    let mut work = Work::default();
    work.schedule(workdir.to_owned(), 0)?;
    let scan = Scan {
        work: Mutex::new(work),
        ready: Condvar::new(),
        stopped: AtomicBool::new(false),
        entries: AtomicUsize::new(0),
    };
    let result = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        let mut failure = None;
        for _ in 0..WORKERS {
            match std::thread::Builder::new()
                .name("skott-scope".into())
                .spawn_scoped(scope, || {
                    let _stop = StopOnExit(&scan);
                    scan.inspect(workdir, preflight, visit)
                }) {
                Ok(handle) => handles.push(handle),
                Err(source) => {
                    scan.stop();
                    failure = Some(io_error(
                        "start writable scope inspection worker",
                        None,
                        source,
                    ));
                    break;
                }
            }
        }
        for handle in handles {
            let result = handle.join().unwrap_or_else(|_| {
                Err(Error::SandboxBuild {
                    reason: "writable scope inspection worker panicked; no request dispatched"
                        .into(),
                })
            });
            if let Err(error) = result
                && failure.is_none()
            {
                failure = Some(error);
            }
        }
        match failure {
            Some(error) => Err(error),
            None => preflight.check(),
        }
    });
    let directories = scan.lock()?.visited.len();
    tracing::debug!(
        entries = scan.entries.load(Ordering::Relaxed),
        directories,
        workers = WORKERS,
        elapsed_ms = started.elapsed().as_millis() as u64,
        failed = result.is_err(),
        "sandbox writable scope inspected"
    );
    result
}

#[derive(Default)]
struct Work {
    stack: Vec<(PathBuf, usize)>,
    visited: BTreeSet<PathBuf>,
    path_bytes: usize,
    active: usize,
}

impl Work {
    fn schedule(&mut self, path: PathBuf, depth: usize) -> Result<()> {
        queue_scope_directory(
            path,
            depth,
            &mut self.stack,
            &mut self.visited,
            &mut self.path_bytes,
        )
    }
}

struct Scan {
    work: Mutex<Work>,
    ready: Condvar,
    stopped: AtomicBool,
    entries: AtomicUsize,
}

impl Scan {
    fn lock(&self) -> Result<MutexGuard<'_, Work>> {
        self.work.lock().map_err(|_| poisoned())
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.ready.notify_all();
    }

    fn next(&self, preflight: &Preflight<'_>) -> Result<Option<(PathBuf, usize)>> {
        let mut work = self.lock()?;
        loop {
            if self.stopped.load(Ordering::Acquire) {
                return Ok(None);
            }
            preflight.check()?;
            if let Some(directory) = work.stack.pop() {
                work.active += 1;
                return Ok(Some(directory));
            }
            if work.active == 0 {
                return Ok(None);
            }
            work = self
                .ready
                .wait_timeout(work, IDLE_POLL)
                .map_err(|_| poisoned())?
                .0;
        }
    }

    fn schedule(&self, path: PathBuf, depth: usize) -> Result<()> {
        self.lock()?.schedule(path, depth)?;
        self.ready.notify_one();
        Ok(())
    }

    fn inspect(
        &self,
        workdir: &Path,
        preflight: &Preflight<'_>,
        visit: &(impl Fn(&Path) -> Result<()> + Sync),
    ) -> Result<()> {
        while let Some((directory, depth)) = self.next(preflight)? {
            visit(&directory)?;
            self.inspect_directory(&directory, depth, workdir, preflight)?;
            self.lock()?.active -= 1;
            self.ready.notify_all();
        }
        Ok(())
    }

    fn inspect_directory(
        &self,
        directory: &Path,
        depth: usize,
        workdir: &Path,
        preflight: &Preflight<'_>,
    ) -> Result<()> {
        preflight.check()?;
        let entries = std::fs::read_dir(directory).map_err(|source| {
            io_error(
                "inspect working directory scope for the sandbox",
                Some(&directory.display().to_string()),
                source,
            )
        })?;
        for entry in entries {
            if self.stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            preflight.check()?;
            admit_scope_entry(&self.entries)?;
            let entry = entry.map_err(|source| {
                io_error(
                    "enumerate working directory scope",
                    Some(&directory.to_string_lossy()),
                    source,
                )
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|source| {
                io_error(
                    "inspect working directory scope for the sandbox",
                    Some(&path.display().to_string()),
                    source,
                )
            })?;
            if file_type.is_dir() {
                self.schedule(path, depth + 1)?;
                continue;
            }
            if !file_type.is_symlink() {
                continue;
            }
            let target = std::fs::read_link(&path).map_err(|source| {
                io_error(
                    "read working directory symbolic link",
                    Some(&path.to_string_lossy()),
                    source,
                )
            })?;
            let resolved = resolve_link_target(&target, &path);
            if target_escapes_scope(&resolved, workdir) {
                return Err(Error::SandboxBuild {
                    reason: format!(
                        "working directory `{}` contains a symbolic link `{}` pointing at
                         `{}` which escapes the writable scope `{}`; a write through such a
                         link could leave the scope, so remove the link, move its target
                         inside the working directory, or narrow the working directory",
                        workdir.display(),
                        path.display(),
                        target.to_string_lossy(),
                        workdir.display(),
                    ),
                });
            }
            let canonical = match std::fs::canonicalize(&path) {
                Ok(canonical) => canonical,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(io_error(
                        "resolve working directory symbolic link",
                        Some(&path.to_string_lossy()),
                        source,
                    ));
                }
            };
            if !canonical.starts_with(workdir) {
                return Err(Error::SandboxBuild {
                    reason: "symbolic link left the working directory during inspection".into(),
                });
            }
            let metadata = std::fs::metadata(&canonical).map_err(|source| {
                io_error(
                    "inspect symbolic link target",
                    Some(&path.to_string_lossy()),
                    source,
                )
            })?;
            if metadata.is_dir() {
                let target_depth = canonical
                    .strip_prefix(workdir)
                    .map_err(|_| Error::SandboxBuild {
                        reason: "symbolic link left the working directory during inspection".into(),
                    })?
                    .components()
                    .count();
                self.schedule(canonical, target_depth)?;
            }
        }
        preflight.check()
    }
}

struct StopOnExit<'a>(&'a Scan);

impl Drop for StopOnExit<'_> {
    fn drop(&mut self) {
        // A failing or panicking worker must also release idle peers.
        self.0.stop();
    }
}

fn poisoned() -> Error {
    Error::SandboxBuild {
        reason: "writable scope inspection state poisoned; no request dispatched".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sav::CancellationToken;

    #[test]
    fn directory_io_errors_are_not_omitted() {
        let root = tempfile::tempdir().unwrap();
        let gone = root.path().join("gone");
        std::fs::create_dir(&gone).unwrap();
        let token = CancellationToken::new();
        let preflight = Preflight::new(&token).stage("workspace_scope");
        let error = check_writable_scope_with(root.path(), &preflight, &|path| {
            if path == gone {
                std::fs::remove_dir(path).unwrap();
            }
            Ok(())
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("inspect working directory scope"),
            "{error}"
        );
        assert!(error.to_string().contains("gone"), "{error}");
    }

    #[test]
    fn worker_panics_wake_waiters_and_fail_inspection() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("child")).unwrap();
        let token = CancellationToken::new();
        let preflight = Preflight::new(&token).stage("workspace_scope");
        let error = check_writable_scope_with(root.path(), &preflight, &|path| {
            assert_eq!(path, root.path(), "injected inspection worker panic");
            Ok(())
        })
        .unwrap_err();
        assert!(error.to_string().contains("worker panicked"), "{error}");
    }

    #[test]
    fn cancellation_during_inspection_stops_owned_workers() {
        let root = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        let preflight = Preflight::new(&token).stage("workspace_scope");
        let error = check_writable_scope_with(root.path(), &preflight, &|_| {
            token.cancel();
            Ok(())
        })
        .unwrap_err();
        assert!(error.to_string().contains("cancelled"), "{error}");
    }

    #[test]
    fn entry_quota_is_shared_across_workers() {
        let count = AtomicUsize::new(super::super::MAX_SCOPE_ENTRIES - 4);
        let successes = std::thread::scope(|scope| {
            let handles = (0..WORKERS)
                .map(|_| scope.spawn(|| admit_scope_entry(&count).is_ok()))
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(|success| *success)
                .count()
        });
        assert_eq!(successes, 4);
    }

    #[test]
    fn concurrent_visitation_stays_within_worker_bound() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..64 {
            std::fs::create_dir(root.path().join(index.to_string())).unwrap();
        }
        let token = CancellationToken::new();
        let preflight = Preflight::new(&token).stage("workspace_scope");
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let visits = AtomicUsize::new(0);
        check_writable_scope_with(root.path(), &preflight, &|_| {
            let current = active.fetch_add(1, Ordering::Relaxed) + 1;
            peak.fetch_max(current, Ordering::Relaxed);
            visits.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(1));
            active.fetch_sub(1, Ordering::Relaxed);
            Ok(())
        })
        .unwrap();
        assert_eq!(visits.load(Ordering::Relaxed), 65);
        assert_eq!(active.load(Ordering::Relaxed), 0);
        assert!(peak.load(Ordering::Relaxed) <= WORKERS);
    }
}
