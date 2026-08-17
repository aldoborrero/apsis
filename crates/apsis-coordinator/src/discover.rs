//! File discovery: a recursive walk of each library, filtered to video files that
//! are size-stable (debounce). inotify is a future add; the periodic walk is the
//! backstop that guarantees convergence (FR-001/002).

use std::path::{Path, PathBuf};
use std::time::Duration;

use apsis_common::is_video;
use walkdir::WalkDir;

/// Video files under `root` that aren't currently being written (mtime older than
/// `debounce`). Unreadable entries are skipped.
pub(crate) fn discover_videos(
    root: &Path,
    extensions: &[String],
    debounce: Duration,
) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|p| is_video(p, extensions))
        .filter(|p| !is_apsis_temp(p)) // an in-flight transcode temp is not a source
        .filter(|p| is_stable(p, debounce))
        .collect()
}

/// A worker's in-progress/orphaned temp (`.apsis-tmp-<ulid>.<ext>`), which looks
/// like a video by extension but must never be reconciled as a source.
fn is_apsis_temp(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(".apsis-tmp-"))
}

/// Remove crash-orphaned `.apsis-tmp-*` files older than `older_than` (an active
/// transcode's temp is recent; a leftover from a crash is old). Returns the count.
pub(crate) fn sweep_temps(root: &Path, older_than: Duration) -> usize {
    WalkDir::new(root)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|p| is_apsis_temp(p) && is_older_than(p, older_than))
        .filter(|p| std::fs::remove_file(p).is_ok())
        .count()
}

fn is_older_than(path: &Path, dur: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age >= dur)
}

/// Stable = last modification is at least `debounce` ago (an actively-written
/// import has a very recent mtime). Unstat-able → not stable (skip this pass).
fn is_stable(path: &Path, debounce: Duration) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    match meta.modified() {
        Ok(mtime) => mtime.elapsed().is_ok_and(|age| age >= debounce),
        Err(_) => true, // no mtime support → don't hold it hostage to debounce
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File, FileTimes, OpenOptions};
    use std::time::UNIX_EPOCH;

    #[test]
    fn returns_stable_videos_only() {
        let dir = std::env::temp_dir().join(format!("apsis-discover-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let old = dir.join("old.mkv");
        fs::write(&old, b"v").unwrap();
        // Backdate its mtime well before the debounce window.
        OpenOptions::new()
            .write(true)
            .open(&old)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH))
            .unwrap();

        let fresh = dir.join("fresh.mkv"); // just written → recent mtime
        fs::write(&fresh, b"v").unwrap();
        fs::write(dir.join("note.txt"), b"x").unwrap(); // not a video
        File::create(dir.join("sub").join("nested.mkv")).ok(); // ensure sub exists
        fs::create_dir_all(dir.join("sub")).unwrap();
        let nested = dir.join("sub").join("nested.mkv");
        fs::write(&nested, b"v").unwrap();
        OpenOptions::new()
            .write(true)
            .open(&nested)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH))
            .unwrap();

        let found = discover_videos(&dir, &[], Duration::from_secs(30));
        // old + nested are stable videos; fresh is within debounce; txt excluded.
        assert!(found.contains(&old), "stable video included");
        assert!(found.contains(&nested), "nested stable video included");
        assert!(!found.contains(&fresh), "fresh video excluded by debounce");
        assert!(!found.iter().any(|p| p.extension().unwrap() == "txt"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn excludes_and_sweeps_orphan_temps() {
        let dir = std::env::temp_dir().join(format!("apsis-sweep-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let temp = dir.join(".apsis-tmp-01ABC.mkv");
        fs::write(&temp, b"partial").unwrap();
        OpenOptions::new()
            .write(true)
            .open(&temp)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH))
            .unwrap();

        // A temp is never discovered as a source, even though `.mkv` looks like video.
        let found = discover_videos(&dir, &[], Duration::from_secs(0));
        assert!(found.is_empty(), "temp excluded from discovery: {found:?}");

        // The sweep removes the old orphan.
        assert_eq!(sweep_temps(&dir, Duration::from_hours(1)), 1);
        assert!(!temp.exists());
        fs::remove_dir_all(&dir).ok();
    }
}
