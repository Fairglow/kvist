use crate::{
    Error, Result,
    fs::{Dir, METADATA_LIMIT, io_error},
    integrity::control,
    unavailable,
    vendoring::{SANDBOX_CARGO_CONFIG_DIRNAME, VENDOR_MANIFEST_FILENAME},
};
use sav::CancellationToken;
use std::path::Path;

pub(crate) const RECOVERY: &str = "vendor-publication";

pub(crate) struct Publication<'a> {
    pub(crate) stage: &'a Dir,
    pub(crate) parent: &'a Dir,
    pub(crate) name: &'a str,
    pub(crate) state: &'a Dir,
    pub(crate) cancellation: &'a CancellationToken,
}

impl Publication<'_> {
    /// Publication is separately injectable so tests can fail a real multi-file commit.
    pub(crate) fn commit(
        self,
        precondition: impl Fn() -> Result<()>,
        metadata: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        if self.stage.entries()? != ["vendor"] {
            return Err(unavailable(
                &self.stage.display,
                "private stage contains unexpected entries",
            ));
        }
        control(self.cancellation)?;
        precondition()?;
        self.parent.still_at_path()?;
        self.state.still_at_path()?;
        if self.state.exists(SANDBOX_CARGO_CONFIG_DIRNAME)? {
            let cargo = self.state.child(SANDBOX_CARGO_CONFIG_DIRNAME)?;
            if cargo.exists("config.toml")? {
                let (old, _) = cargo.read("config.toml", METADATA_LIMIT)?;
                self.stage.atomic("old-config.toml", &old)?;
            }
        }
        if self.state.exists(VENDOR_MANIFEST_FILENAME)? {
            let (old, _) = self.state.read(VENDOR_MANIFEST_FILENAME, METADATA_LIMIT)?;
            self.stage.atomic("old-manifest.json", &old)?;
        }
        let recovery_path = self.state.display.join(RECOVERY);
        std::fs::create_dir(self.state.path().join(RECOVERY))
            .map_err(|e| io_error("create publication recovery fence", &recovery_path, e))?;
        let recovery = self.state.child(RECOVERY)?;
        let evidence = format!(
            "stage={}\ndestination={}\nold=old-vendor\nold-config=old-config.toml\nold-manifest=old-manifest.json\n",
            self.stage.display.display(),
            self.parent.display.join(self.name).display()
        );
        let mut moved = false;
        let mut published = false;
        let result = (|| -> Result<()> {
            recovery.atomic("state.txt", evidence.as_bytes())?;
            self.state.sync()?;
            control(self.cancellation)?;
            precondition()?;
            if self.parent.exists(self.name)? {
                self.parent.child(self.name)?;
                rename(
                    &self.parent.path().join(self.name),
                    &self.stage.path().join("old-vendor"),
                )?;
                moved = true;
                self.parent.sync()?;
                self.stage.sync()?;
            }
            control(self.cancellation)?;
            rename(
                &self.stage.path().join("vendor"),
                &self.parent.path().join(self.name),
            )?;
            published = true;
            self.parent.sync()?;
            self.stage.sync()?;
            metadata()?;
            control(self.cancellation)?;
            precondition()?;
            // Metadata has fully committed before removing the readiness fence.
            clear(&recovery, self.state)?;
            Ok(())
        })();
        if let Err(source) = result {
            if !moved && !published {
                // No public material changed: keep the previous state usable.
                if clear(&recovery, self.state).is_ok() {
                    return Err(source);
                }
            }
            // If clearing the fence itself failed after commit, recreate evidence
            // when possible. Never delete retained old material on this path.
            if !self.state.exists(RECOVERY).unwrap_or(true)
                && std::fs::create_dir(self.state.path().join(RECOVERY)).is_ok()
                && let Ok(fence) = self.state.child(RECOVERY)
            {
                let _ = fence.atomic("state.txt", evidence.as_bytes());
                let _ = self.state.sync();
            }
            return Err(Error::Publication {
                published,
                recovery: recovery_path,
                source: Box::new(source),
            });
        }
        Ok(())
    }
}

fn rename(from: &Path, to: &Path) -> Result<()> {
    std::fs::rename(from, to).map_err(|e| io_error("rename dependency material", to, e))
}

fn clear(recovery: &Dir, state: &Dir) -> Result<()> {
    if recovery.exists("state.txt")? {
        std::fs::remove_file(recovery.path().join("state.txt"))
            .map_err(|e| io_error("clear committed recovery evidence", &recovery.display, e))?;
    }
    std::fs::remove_dir(state.path().join(RECOVERY))
        .map_err(|e| io_error("clear committed publication fence", &recovery.display, e))?;
    state.sync()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{rust, vendoring::offline_cargo_config};

    #[test]
    fn metadata_failure_retains_old_registry_and_fences_readiness() {
        let project = tempfile::Builder::new()
            .prefix(".dependencies-publication-test-")
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap();
        let root = Dir::open(project.path()).unwrap();
        let state = root.ensure_child(".kvist").unwrap();
        let vendor = state.ensure_child("vendored").unwrap();
        std::fs::write(vendor.path().join("old"), b"previous registry").unwrap();
        std::fs::write(state.path().join("vendoring-v1.json"), b"previous manifest").unwrap();
        let cargo = state.ensure_child("sandbox-cargo").unwrap();
        cargo
            .atomic(
                "config.toml",
                offline_cargo_config("/workspace/vendored").as_bytes(),
            )
            .unwrap();
        std::fs::write(root.path().join("Cargo.lock"), "version=4\n").unwrap();
        let stage = state.ensure_child("private-stage").unwrap();
        let new = stage.ensure_child("vendor").unwrap();
        std::fs::write(new.path().join("new"), b"new registry").unwrap();
        let token = CancellationToken::new();
        let error = Publication {
            stage: &stage,
            parent: &state,
            name: "vendored",
            state: &state,
            cancellation: &token,
        }
        .commit(
            || Ok(()),
            || {
                cargo.atomic("config.toml", b"partial resolver commit")?;
                Err(unavailable(
                    project.path(),
                    "injected manifest publication failure",
                ))
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            Error::Publication {
                published: true,
                ..
            }
        ));
        assert_eq!(
            std::fs::read(stage.path().join("old-vendor/old")).unwrap(),
            b"previous registry"
        );
        assert_eq!(
            std::fs::read(state.path().join("vendoring-v1.json")).unwrap(),
            b"previous manifest"
        );
        assert_eq!(
            std::fs::read(stage.path().join("old-manifest.json")).unwrap(),
            b"previous manifest"
        );
        assert_eq!(
            std::fs::read(stage.path().join("old-config.toml")).unwrap(),
            offline_cargo_config("/workspace/vendored").as_bytes()
        );
        assert!(state.exists(RECOVERY).unwrap());
        assert!(rust::check(project.path(), &token).is_err());
    }

    #[test]
    fn cancellation_before_any_rename_keeps_prior_material() {
        let project = tempfile::Builder::new()
            .prefix(".dependencies-publication-test-")
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap();
        let root = Dir::open(project.path()).unwrap();
        let state = root.ensure_child(".kvist").unwrap();
        let old = state.ensure_child("vendored").unwrap();
        std::fs::write(old.path().join("old"), b"previous").unwrap();
        let stage = state.ensure_child("private-stage").unwrap();
        stage.ensure_child("vendor").unwrap();
        let token = CancellationToken::new();
        let checks = std::cell::Cell::new(0);
        let error = Publication {
            stage: &stage,
            parent: &state,
            name: "vendored",
            state: &state,
            cancellation: &token,
        }
        .commit(
            || {
                checks.set(checks.get() + 1);
                if checks.get() == 2 {
                    token.cancel();
                    return Err(Error::Cancelled);
                }
                Ok(())
            },
            || panic!("cancelled before publication"),
        )
        .unwrap_err();
        assert!(matches!(error, Error::Cancelled));
        assert_eq!(std::fs::read(old.path().join("old")).unwrap(), b"previous");
        assert!(!state.exists(RECOVERY).unwrap());
    }
}
