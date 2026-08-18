//! Test-only fixtures shared across the worker's unit + integration tests
//! (`testkit` feature — never compiled into a release build).
//!
//! Every worker test needs a throwaway h264 clip to transcode; this is the one
//! generator instead of a hand-rolled `ffmpeg -f lavfi` in each.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Whether `ffmpeg` is on `PATH`. Tests that need a real transcode call this and
/// skip (return early) when it isn't, so the suite still passes on a bare box.
#[must_use]
pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

/// Generate a tiny h264 clip at `dir/name` — `testsrc`, `secs` long, `size` like
/// `"128x128"`. Returns the path.
///
/// # Panics
/// If ffmpeg can't be spawned or exits non-zero — a broken fixture is a test bug,
/// not a runtime condition to handle. Guard call sites with [`ffmpeg_available`].
#[must_use]
pub fn sample_h264(dir: &Path, name: &str, secs: u32, size: &str) -> PathBuf {
    let path = dir.join(name);
    let out = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=d={secs}:s={size}"),
            "-c:v",
            "libx264",
        ])
        .arg(&path)
        .output()
        .expect("spawn ffmpeg");
    assert!(
        out.status.success(),
        "ffmpeg clip generation failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    path
}
