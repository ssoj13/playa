//! Snapshot the active comp's currently-displayed decoded frame to disk,
//! auto-import the file into the project, and produce a base64 data URL
//! suitable for fal `image_url` (i2v reference image).
//!
//! Uses [`PlayaApp::capture_raw_frame`] under the hood — the same JPEG
//! bytes the API screenshot path emits. This is the **composited** comp
//! output at native resolution, post-tonemap (HDR → ACES → U8), before
//! any egui viewport rendering. UI chrome is not involved.
//!
//! Output naming: `{cache_dir}/playa/snapshots/{timestamp}_{comp_uuid}.jpg`.
//! Falls back to `std::env::temp_dir()/playa-snapshots/` if `dirs_next`
//! can't resolve a cache dir.

use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose;
use playa_engine::entities::frame::FrameStatus;

use crate::app::PlayaApp;

/// Outcome of a snapshot operation. Caller (`update()`) typically writes
/// `data_url` into the SubmitDialog's `image_url` field and lets
/// `load_sequences` import the disk file.
pub struct SnapshotResult {
    /// Disk path the JPEG was written to. Already imported into the
    /// project via `load_sequences` by the time this returns.
    pub path: PathBuf,
    /// `data:image/jpeg;base64,...` URL suitable for fal `image_url`.
    pub data_url: String,
}

impl PlayaApp {
    /// Snapshot the active comp's current decoded frame.
    ///
    /// Steps:
    /// 1. Refuse if no frame in viewport or status != Loaded (avoid
    ///    capturing placeholder/stale pixels).
    /// 2. Call [`Self::capture_raw_frame`] → JPEG bytes (compositor
    ///    output at native res, post-tonemap).
    /// 3. Write to `{cache_dir}/playa/snapshots/{ts}_{comp}.jpg`.
    /// 4. Import the file via `load_sequences` so it appears as a node
    ///    in the project (user can drag onto a comp / use as i2v
    ///    reference / reopen later).
    /// 5. Build a `data:image/jpeg;base64,...` URL.
    pub fn snapshot_current_frame(&mut self) -> Result<SnapshotResult, String> {
        // Step 1: readiness guard.
        let Some(frame) = self.frame.as_ref() else {
            return Err("no frame in viewport".to_string());
        };
        let status = frame.status();
        if status != FrameStatus::Loaded {
            return Err(format!(
                "frame not ready ({:?}) — wait for cache to settle",
                status
            ));
        }

        // Step 2: JPEG bytes via the existing capture path.
        let jpeg_bytes = self.capture_raw_frame()?;

        // Step 3: write to disk.
        let snapshot_dir = dirs_next::cache_dir()
            .map(|d| d.join("playa").join("snapshots"))
            .unwrap_or_else(|| std::env::temp_dir().join("playa-snapshots"));
        std::fs::create_dir_all(&snapshot_dir)
            .map_err(|e| format!("create snapshot dir failed: {e}"))?;
        let ts = chrono_secs();
        let comp_tag = self
            .player
            .active_comp()
            .map(|u| u.to_string()[..8].to_string())
            .unwrap_or_else(|| "noactive".to_string());
        let filename = format!("{ts}_{comp_tag}.jpg");
        let path = snapshot_dir.join(filename);
        std::fs::write(&path, &jpeg_bytes)
            .map_err(|e| format!("write snapshot {}: {e}", path.display()))?;
        log::info!(
            "Snapshot written to {} ({} bytes)",
            path.display(),
            jpeg_bytes.len()
        );

        // Step 4: import as a project node so the user sees it.
        if let Err(e) = self.load_sequences(vec![path.clone()]) {
            // Soft-fail: file is on disk + we still return data_url below.
            log::warn!("Snapshot auto-import failed (file is on disk): {e}");
        }

        // Step 5: data URL.
        let b64 = general_purpose::STANDARD.encode(&jpeg_bytes);
        let data_url = format!("data:image/jpeg;base64,{b64}");
        Ok(SnapshotResult { path, data_url })
    }
}

/// UTC seconds since epoch, formatted as a sortable filename prefix.
/// Avoids a chrono dep — `SystemTime` is enough for filenames.
fn chrono_secs() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
