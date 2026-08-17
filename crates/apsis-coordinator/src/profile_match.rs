//! Select a file's library by the **longest matching** configured path (FR-003),
//! at a path-segment boundary so `/hdd/tv` never matches `/hdd/tvshows`.

use apsis_common::config::Library;

/// The library whose `path` is the longest prefix of `file`, or `None`.
pub(crate) fn match_library<'a>(libraries: &'a [Library], file: &str) -> Option<&'a Library> {
    libraries
        .iter()
        .filter(|lib| covers(&lib.path, file))
        .max_by_key(|lib| lib.path.trim_end_matches('/').len())
}

fn covers(lib_path: &str, file: &str) -> bool {
    let base = lib_path.trim_end_matches('/');
    match file.strip_prefix(base) {
        Some(rest) => rest.is_empty() || rest.starts_with('/'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(name: &str, path: &str) -> Library {
        serde_json::from_str(&format!(
            r#"{{"name":"{name}","path":"{path}","profile":"{name}"}}"#
        ))
        .unwrap()
    }

    #[test]
    fn longest_prefix_wins() {
        let libs = [lib("tv", "/hdd/tv"), lib("anime", "/hdd/tv/anime")];
        assert_eq!(
            match_library(&libs, "/hdd/tv/anime/show/e01.mkv")
                .unwrap()
                .name,
            "anime"
        );
        assert_eq!(
            match_library(&libs, "/hdd/tv/drama/e01.mkv").unwrap().name,
            "tv"
        );
    }

    #[test]
    fn segment_boundary_and_no_match() {
        let libs = [lib("tv", "/hdd/tv")];
        // `/hdd/tvshows` must NOT match the `/hdd/tv` library.
        assert!(match_library(&libs, "/hdd/tvshows/e01.mkv").is_none());
        assert!(match_library(&libs, "/other/e01.mkv").is_none());
        // exact directory also covered.
        assert!(match_library(&libs, "/hdd/tv").is_some());
    }
}
