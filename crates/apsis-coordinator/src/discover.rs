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
        .filter(|p| is_stable(p, debounce))
        .collect()
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
}
