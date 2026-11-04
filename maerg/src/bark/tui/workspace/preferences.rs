//! Bounded private component shortcuts; never task or approval configuration.

use std::{io, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Preferences {
    #[serde(default)]
    pub pinned: Vec<String>,
    #[serde(default)]
    pub recent: Vec<String>,
}

impl Preferences {
    fn validate(&self) -> io::Result<()> {
        for entries in [&self.pinned, &self.recent] {
            if entries.len() > 12
                || entries.iter().any(|entry| {
                    entry.is_empty()
                        || entry.len() > 4096
                        || entry.chars().any(char::is_control)
                        || Path::new(entry).components().any(|part| {
                            !matches!(
                                part,
                                std::path::Component::Normal(_) | std::path::Component::CurDir
                            )
                        })
                })
            {
                return Err(io::Error::other(
                    "invalid component shortcuts: at most 12 relative component paths per list",
                ));
            }
        }
        Ok(())
    }

    pub fn load(project: &Path) -> io::Result<Self> {
        let path = project.join(".kvist/transcript/workspace.json");
        match super::super::storage::read_private(&path, 16_384) {
            Ok(text) => {
                let preferences: Self = serde_json::from_str(&text)
                    .map_err(|error| io::Error::other(format!("{}: {error}", path.display())))?;
                preferences.validate()?;
                Ok(preferences)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, project: &Path) -> io::Result<()> {
        self.validate()?;
        let text = serde_json::to_string(self).map_err(io::Error::other)?;
        if text.len() > 16_384 {
            return Err(io::Error::other("component shortcuts exceed 16384 bytes"));
        }
        let path = project.join(".kvist/transcript/workspace.json");
        match super::super::storage::read_private(&path, 16_384) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        crate::file_io::replace_file_atomically(&path, &text).map_err(io::Error::other)
    }

    pub fn visit(&mut self, component: &str) {
        self.recent.retain(|entry| entry != component);
        self.recent.insert(0, component.to_owned());
        self.recent.truncate(12);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_bounded_private_and_restored() {
        let dir = tempfile::tempdir().unwrap();
        let _transcript = super::super::super::storage::Transcript::open(dir.path()).unwrap();
        let mut preferences = Preferences::default();
        preferences.pinned.push("src".into());
        for index in 0..20 {
            preferences.visit(&format!("component-{index}"));
        }
        preferences.save(dir.path()).unwrap();
        let restored = Preferences::load(dir.path()).unwrap();
        assert_eq!(restored.pinned, ["src"]);
        assert_eq!(restored.recent.len(), 12);
        assert_eq!(restored.recent[0], "component-19");
        preferences.pinned.push("../other".into());
        assert!(preferences.save(dir.path()).is_err());
        assert_eq!(Preferences::load(dir.path()).unwrap().pinned, ["src"]);
    }
}
