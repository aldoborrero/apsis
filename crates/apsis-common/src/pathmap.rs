//! Coordinator-path → local-mount translation (worker-side).
//!
//! Identity on rhea; real on sirius/WSL where the NFS mount point differs
//! (spec 003). Longest-matching prefix wins, at a path-segment boundary so
//! `/hdd` never matches `/hddmore`.

use std::collections::BTreeMap;

/// A set of `coordinator_prefix → local_prefix` rules.
#[derive(Debug, Clone, Default)]
pub struct PathMap {
    rules: BTreeMap<String, String>,
}

/// Does `key` match `path` at a segment boundary (whole path or `key/...`)?
fn boundary_match<'a>(key: &str, path: &'a str) -> Option<&'a str> {
    let key = key.trim_end_matches('/');
    match path.strip_prefix(key) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => Some(rest),
        _ => None,
    }
}

impl PathMap {
    #[must_use]
    pub fn new(rules: BTreeMap<String, String>) -> Self {
        Self { rules }
    }

    /// Translate a coordinator-space path to this host's local path. Falls back
    /// to the input unchanged when no rule matches (identity).
    #[must_use]
    pub fn translate(&self, path: &str) -> String {
        let best = self
            .rules
            .iter()
            .filter_map(|(k, v)| boundary_match(k, path).map(|rest| (k.len(), v, rest)))
            .max_by_key(|(klen, _, _)| *klen);
        match best {
            Some((_, local, rest)) => format!("{}{}", local.trim_end_matches('/'), rest),
            None => path.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> PathMap {
        PathMap::new(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    #[test]
    fn identity_when_no_rule() {
        assert_eq!(map(&[]).translate("/hdd/x.mkv"), "/hdd/x.mkv");
    }

    #[test]
    fn longest_prefix_wins_at_boundary() {
        let m = map(&[("/hdd", "/mnt/rhea-hdd"), ("/hdd/tv", "/fast/tv")]);
        assert_eq!(
            m.translate("/hdd/movies/a.mkv"),
            "/mnt/rhea-hdd/movies/a.mkv"
        );
        assert_eq!(m.translate("/hdd/tv/b.mkv"), "/fast/tv/b.mkv");
    }

    #[test]
    fn does_not_match_partial_segment() {
        let m = map(&[("/hdd", "/mnt/rhea-hdd")]);
        // "/hddmore" must NOT be rewritten by the "/hdd" rule.
        assert_eq!(m.translate("/hddmore/x.mkv"), "/hddmore/x.mkv");
        // exact match maps to the local root.
        assert_eq!(m.translate("/hdd"), "/mnt/rhea-hdd");
    }
}
