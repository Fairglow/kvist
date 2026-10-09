//! Session manager for Blad.
//!
//! Manages the lifecycle of session directories, manifests, and message files.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

static NEXT_SESSION: AtomicUsize = AtomicUsize::new(0);

/// Session status values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Streaming,
    Completed,
    Cancelled,
    Failed,
}

/// Message entry in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub sequence: usize,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition: Option<String>,
    pub media_type: String,
    pub path: String,
    pub bytes: usize,
    pub complete: bool,
    pub truncated: bool,
}

/// Session manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionManifest {
    pub schema_version: u32,
    pub session_id: String,
    pub created_at: String,
    pub status: SessionStatus,
    pub entry_count: usize,
    pub entries: Vec<ManifestEntry>,
}

/// Session manager.
pub struct SessionManager {
    session_id: String,
    session_dir: PathBuf,
    manifest_path: PathBuf,
    manifest: SessionManifest,
}

impl SessionManager {
    /// Create a new session manager with a unique session ID.
    pub fn new(base_dir: &Path) -> io::Result<Self> {
        let session_id = Self::generate_session_id();
        let session_dir = base_dir.join(&session_id);
        let messages_dir = session_dir.join("messages");

        fs::create_dir_all(&messages_dir)?;

        let created_at = Self::rfc3339_now();

        let manifest = SessionManifest {
            schema_version: 1,
            session_id: session_id.clone(),
            created_at,
            status: SessionStatus::Streaming,
            entry_count: 0,
            entries: Vec::new(),
        };

        let manifest_path = session_dir.join("manifest.json");
        Self::write_manifest(&manifest_path, &manifest)?;

        Ok(Self {
            session_id,
            session_dir,
            manifest_path,
            manifest,
        })
    }

    /// Generate a unique session ID.
    fn generate_session_id() -> String {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before Unix epoch");
        let seconds = elapsed.as_secs();
        let counter = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();

        // Fixed-width UTC timestamp format
        let time = chrono::DateTime::from_timestamp(seconds as i64, 0)
            .expect("invalid timestamp")
            .format("%Y%m%dT%H%M%SZ");

        format!("{}-{}-{}", time, pid, counter)
    }

    /// Get RFC3339 formatted current time.
    fn rfc3339_now() -> String {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before Unix epoch");
        chrono::DateTime::from_timestamp(elapsed.as_secs() as i64, 0)
            .expect("invalid timestamp")
            .to_rfc3339()
    }

    /// Write the manifest atomically.
    fn write_manifest(path: &Path, manifest: &SessionManifest) -> io::Result<()> {
        let json = serde_json::to_string_pretty(manifest)?;
        let mut file = File::create(path)?;
        file.write_all(json.as_bytes())?;
        file.flush()?;
        Ok(())
    }

    /// Add a new message entry to the session.
    pub fn add_message(
        &mut self,
        kind: &str,
        disposition: Option<&str>,
        content: &str,
        complete: bool,
    ) -> io::Result<usize> {
        let sequence = self.manifest.entry_count + 1;
        let entry_id = format!("msg-{:06}", sequence);
        let path = format!("messages/{:06}.md", sequence);

        // Write message file
        let message_path = self.session_dir.join(&path);
        let mut file = File::create(&message_path)?;
        file.write_all(content.as_bytes())?;
        file.flush()?;

        // Create manifest entry
        let entry = ManifestEntry {
            id: entry_id,
            sequence,
            kind: kind.to_string(),
            disposition: disposition.map(|s| s.to_string()),
            media_type: "text/markdown".to_string(),
            path: path.to_string(),
            bytes: content.len(),
            complete,
            truncated: !complete,
        };

        self.manifest.entries.push(entry);
        self.manifest.entry_count = sequence;

        // Update manifest
        Self::write_manifest(&self.manifest_path, &self.manifest)?;

        Ok(sequence)
    }

    /// Update an existing message entry (for streaming updates).
    pub fn update_message(
        &mut self,
        sequence: usize,
        content: &str,
        complete: bool,
    ) -> io::Result<()> {
        // Find the entry
        let entry = self
            .manifest
            .entries
            .iter_mut()
            .find(|e| e.sequence == sequence);

        if entry.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("message {} not found", sequence),
            ));
        }

        let entry = entry.unwrap();

        // Write updated message file
        let path = format!("messages/{:06}.md", sequence);
        let message_path = self.session_dir.join(&path);
        let mut file = File::create(&message_path)?;
        file.write_all(content.as_bytes())?;
        file.flush()?;

        // Update manifest entry
        entry.bytes = content.len();
        entry.complete = complete;
        entry.truncated = !complete;

        // Update manifest
        Self::write_manifest(&self.manifest_path, &self.manifest)?;

        Ok(())
    }

    /// Complete the session.
    pub fn complete(&mut self) -> io::Result<()> {
        self.manifest.status = SessionStatus::Completed;
        Self::write_manifest(&self.manifest_path, &self.manifest)?;
        Ok(())
    }

    /// Cancel the session.
    pub fn cancel(&mut self) -> io::Result<()> {
        self.manifest.status = SessionStatus::Cancelled;
        Self::write_manifest(&self.manifest_path, &self.manifest)?;
        Ok(())
    }

    /// Mark session as failed.
    pub fn fail(&mut self) -> io::Result<()> {
        self.manifest.status = SessionStatus::Failed;
        Self::write_manifest(&self.manifest_path, &self.manifest)?;
        Ok(())
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the session directory path.
    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    /// Get the manifest path.
    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    /// Get the manifest.
    pub fn manifest(&self) -> &SessionManifest {
        &self.manifest
    }

    /// Load a session by ID.
    pub fn load(base_dir: &Path, session_id: &str) -> io::Result<Self> {
        let session_dir = base_dir.join(session_id);
        let manifest_path = session_dir.join("manifest.json");

        let json = fs::read_to_string(&manifest_path)?;
        let manifest: SessionManifest = serde_json::from_str(&json)?;

        if manifest.session_id != session_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session ID mismatch in manifest",
            ));
        }

        Ok(Self {
            session_id: session_id.to_string(),
            session_dir,
            manifest_path,
            manifest,
        })
    }
}

impl Drop for SessionManager {
    fn drop(&mut self) {
        // Flush manifest on drop
        if let Err(e) = Self::write_manifest(&self.manifest_path, &self.manifest) {
            eprintln!("Warning: failed to write manifest on drop: {}", e);
        }
    }
}
