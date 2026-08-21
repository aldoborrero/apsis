//! Declarative config (TOML in git), loaded via figment and validated with
//! garde. Loading is **fail-fast**: an invalid config yields an error and the
//! binary refuses to start, so the last-good deployment keeps running (FR-012).
//!
//! Env layering is intentionally *not* used: figment's `Env::prefixed` emits the
//! whole `APSIS_*` namespace as top-level keys, which (a) can't reach nested
//! fields and (b) makes an unrelated `APSIS_*` var trip `deny_unknown_fields` and
//! refuse startup. Config is TOML files in git; overrides edit the file.
//!
//! `scheduler.toml` drives the coordinator; `worker.toml` the worker. Profiles
//! are `apsis_engine::Profile`; garde validation here covers the apsis-level
//! fields, while the engine enforces its own invariants at deserialize
//! (`video.quality` range, and `deny_unknown_fields` on every profile sub-struct
//! so a typo inside a `[profiles.*]` table fails loudly rather than being dropped
//! to a default).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use apsis_engine::{HardwareConfig, Profile};
use figment::Figment;
use figment::providers::{Format, Toml};
use garde::Validate;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigError {
    // Boxed: figment::Error / garde::Report are large; keep ConfigError small
    // (clippy::result_large_err) since it rides in every loader's Result.
    #[error("config parse error: {0}")]
    Figment(Box<figment::Error>),
    #[error("config validation failed: {0}")]
    Invalid(Box<garde::Report>),
    #[error("library {library:?} references unknown profile {profile:?}")]
    UnknownProfile { library: String, profile: String },
    #[error("at least one {0} must be configured")]
    Empty(&'static str),
    #[error("duplicate library name {0:?}")]
    DuplicateLibrary(String),
    #[error("library {library:?} path {path:?} must be absolute")]
    RelativePath { library: String, path: String },
    #[error("verify.max_size_ratio must be finite and >= 1.0, got {0}")]
    BadRatio(f64),
    #[error("profile {profile:?}: {message}")]
    InvalidRule { profile: String, message: String },
}

impl From<figment::Error> for ConfigError {
    fn from(e: figment::Error) -> Self {
        Self::Figment(Box::new(e))
    }
}

impl From<garde::Report> for ConfigError {
    fn from(e: garde::Report) -> Self {
        Self::Invalid(Box::new(e))
    }
}

// --- scheduler.toml (coordinator) ---

#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub struct SchedulerConfig {
    #[garde(dive)]
    #[serde(rename = "library", default)] // TOML `[[library]]`; empty → Empty("library")
    pub libraries: Vec<Library>,
    /// `name -> Profile`; the engine validated each on deserialize.
    pub profiles: BTreeMap<String, Profile>,
    #[serde(default)]
    pub reconcile: Reconcile,
}

#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub struct Library {
    #[garde(length(min = 1))]
    pub name: String,
    #[garde(length(min = 1))]
    pub path: String,
    #[garde(length(min = 1))]
    pub profile: String,
    /// Overrides the default video extensions (empty = defaults).
    #[serde(default)]
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reconcile {
    #[serde(with = "humantime_serde", default = "d_scan_interval")]
    pub scan_interval: Duration,
    #[serde(with = "humantime_serde", default = "d_debounce")]
    pub debounce: Duration,
    #[serde(default = "d_true")]
    pub inotify: bool,
}

impl Default for Reconcile {
    fn default() -> Self {
        Self {
            scan_interval: d_scan_interval(),
            debounce: d_debounce(),
            inotify: true,
        }
    }
}

// --- worker.toml (worker) ---

#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub struct WorkerConfig {
    #[garde(range(min = 1, max = 256))]
    #[serde(default = "d_concurrency")]
    pub concurrency: u32,
    /// Coordinator path → local mount (identity on rhea).
    #[serde(default)]
    pub path_map: BTreeMap<String, String>,
    #[serde(default = "d_ffmpeg")]
    pub ffmpeg: PathBuf,
    #[serde(default = "d_ffprobe")]
    pub ffprobe: PathBuf,
    /// Kill ffmpeg if it emits no `-progress` output for this long (a hung VAAPI/
    /// VCN driver would otherwise heartbeat the lease forever and wedge the worker).
    #[serde(with = "humantime_serde", default = "d_stall_timeout")]
    pub stall_timeout: Duration,
    #[serde(default)]
    pub verify: VerifyConfig,
    /// `JetStream` consumer lease/retry tuning (`[consumer]`). Defaults match the
    /// production 30-min lease / 4 deliveries / 1-5-15-min backoff; operators (and
    /// the gated crash tests) override it for a faster lease/redelivery.
    #[garde(dive)]
    #[serde(default)]
    pub consumer: ConsumerConfig,
    #[garde(dive)]
    #[serde(rename = "backend", default)] // TOML `[[backend]]`; empty → Empty("backend")
    pub backends: Vec<BackendConfig>,
}

/// `[consumer]` — the worker's pull-consumer lease/retry knobs, mapped to a
/// [`crate::nats::ConsumerTuning`]. Split out so lease timing is operator-tunable
/// (and testable) instead of hardcoded.
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub struct ConsumerConfig {
    /// Lease per delivery; a legitimately long encode heartbeats within it so it
    /// isn't redelivered mid-run (contract §delivery).
    #[serde(with = "humantime_serde", default = "d_ack_wait")]
    pub ack_wait: Duration,
    /// Deliveries before a job is dead-lettered `Failed@version` (FR-009).
    #[garde(range(min = 1))]
    #[serde(default = "d_max_deliver")]
    pub max_deliver: i64,
    /// Redelivery backoff schedule (humantime strings, e.g. `["1m","5m"]`); an
    /// empty list means immediate redelivery.
    #[serde(default = "d_backoff", deserialize_with = "de_durations")]
    pub backoff: Vec<Duration>,
}

impl Default for ConsumerConfig {
    fn default() -> Self {
        Self {
            ack_wait: d_ack_wait(),
            max_deliver: d_max_deliver(),
            backoff: d_backoff(),
        }
    }
}

impl ConsumerConfig {
    /// Build the runtime [`crate::nats::ConsumerTuning`]; `max_ack_pending` mirrors
    /// the worker's `concurrency` (the broker-side bound of the local semaphore).
    #[must_use]
    pub fn tuning(&self, concurrency: u32) -> crate::nats::ConsumerTuning {
        crate::nats::ConsumerTuning {
            ack_wait: self.ack_wait,
            max_deliver: self.max_deliver,
            max_ack_pending: i64::from(concurrency.max(1)),
            backoff: self.backoff.clone(),
        }
    }
}

/// Deserialize a list of humantime strings into `Vec<Duration>` (so a bad string
/// fails config load, not at consumer-bind time).
fn de_durations<'de, D>(d: D) -> Result<Vec<Duration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Vec::<String>::deserialize(d)?;
    raw.iter()
        .map(|s| humantime::parse_duration(s).map_err(serde::de::Error::custom))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Vaapi,
    Cpu,
}

#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub struct BackendConfig {
    pub kind: BackendKind,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub hardware: HardwareConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyConfig {
    #[serde(with = "humantime_serde", default = "d_dur_tol")]
    pub duration_tolerance: Duration,
    /// Output must not exceed `input * max_size_ratio`. Validated finite and ≥ 1.0
    /// in `worker_from` — garde's `range` can't reject `NaN` (`NaN < 1.0` is false).
    #[serde(default = "d_max_size_ratio")]
    pub max_size_ratio: f64,
    #[serde(default)]
    pub deep_verify: bool,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            duration_tolerance: d_dur_tol(),
            max_size_ratio: d_max_size_ratio(),
            deep_verify: false,
        }
    }
}

// --- defaults ---

fn d_scan_interval() -> Duration {
    Duration::from_mins(5)
}
fn d_debounce() -> Duration {
    Duration::from_mins(1)
}
fn d_true() -> bool {
    true
}
fn d_concurrency() -> u32 {
    1
}
fn d_ffmpeg() -> PathBuf {
    PathBuf::from("ffmpeg")
}
fn d_ffprobe() -> PathBuf {
    PathBuf::from("ffprobe")
}
fn d_stall_timeout() -> Duration {
    Duration::from_mins(2)
}
fn d_ack_wait() -> Duration {
    Duration::from_mins(30)
}
fn d_max_deliver() -> i64 {
    4
}
fn d_backoff() -> Vec<Duration> {
    vec![
        Duration::from_mins(1),
        Duration::from_mins(5),
        Duration::from_mins(15),
    ]
}
fn d_dur_tol() -> Duration {
    Duration::from_secs(1)
}
fn d_max_size_ratio() -> f64 {
    1.5
}

// --- loaders (fail-fast) ---

fn scheduler_from(fig: &Figment) -> Result<SchedulerConfig, ConfigError> {
    let cfg: SchedulerConfig = fig.extract()?;
    cfg.validate()?;
    if cfg.libraries.is_empty() {
        return Err(ConfigError::Empty("library"));
    }
    let mut seen = std::collections::HashSet::new();
    for lib in &cfg.libraries {
        if !seen.insert(lib.name.as_str()) {
            return Err(ConfigError::DuplicateLibrary(lib.name.clone()));
        }
        // Paths are absolute prefixes (profile match + worker path_map depend on it).
        if !std::path::Path::new(&lib.path).is_absolute() {
            return Err(ConfigError::RelativePath {
                library: lib.name.clone(),
                path: lib.path.clone(),
            });
        }
        if !cfg.profiles.contains_key(&lib.profile) {
            return Err(ConfigError::UnknownProfile {
                library: lib.name.clone(),
                profile: lib.profile.clone(),
            });
        }
    }
    // Fail-fast on invalid CEL profile rules (FR-013): compile + canary-evaluate every
    // rule so a syntax error / unknown context field / static out-of-range refuses to
    // start, rather than surfacing per-file at reconcile.
    for (name, profile) in &cfg.profiles {
        apsis_engine::validate_rules(profile, &profile.rules).map_err(|e| {
            ConfigError::InvalidRule {
                profile: name.clone(),
                message: e.to_string(),
            }
        })?;
    }
    Ok(cfg)
}

fn worker_from(fig: &Figment) -> Result<WorkerConfig, ConfigError> {
    let cfg: WorkerConfig = fig.extract()?;
    cfg.validate()?;
    if cfg.backends.is_empty() {
        return Err(ConfigError::Empty("backend"));
    }
    let ratio = cfg.verify.max_size_ratio;
    if !ratio.is_finite() || ratio < 1.0 {
        return Err(ConfigError::BadRatio(ratio));
    }
    Ok(cfg)
}

/// Load + validate `scheduler.toml`.
///
/// # Errors
/// Parse, validation, empty-libraries, or unknown-profile failures (fail-fast).
pub fn load_scheduler(path: &Path) -> Result<SchedulerConfig, ConfigError> {
    scheduler_from(&Figment::new().merge(Toml::file(path)))
}

/// Load + validate `worker.toml`.
///
/// # Errors
/// Parse, validation, empty-backends, or bad-ratio failures (fail-fast).
pub fn load_worker(path: &Path) -> Result<WorkerConfig, ConfigError> {
    worker_from(&Figment::new().merge(Toml::file(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHED: &str = r#"
        [[library]]
        name = "tv"
        path = "/hdd/tv"
        profile = "tv"

        [profiles.tv.video]
        codec = "hevc"
        skip_codecs = ["hevc", "av1"]
        [profiles.tv.audio]
        [profiles.tv.subtitles]
        [profiles.tv.output]
        container = "mkv"
    "#;

    #[test]
    fn scheduler_loads_with_defaults() {
        let cfg = scheduler_from(&Figment::new().merge(Toml::string(SCHED))).unwrap();
        assert_eq!(cfg.libraries.len(), 1);
        assert_eq!(cfg.libraries[0].path, "/hdd/tv");
        // reconcile defaults applied
        assert_eq!(cfg.reconcile.scan_interval, Duration::from_mins(5));
        assert_eq!(cfg.reconcile.debounce, Duration::from_mins(1));
        assert!(cfg.reconcile.inotify);
    }

    #[test]
    fn unknown_profile_is_rejected() {
        let toml = r#"
            [[library]]
            name = "tv"
            path = "/hdd/tv"
            profile = "missing"
            [profiles.tv.video]
            codec = "hevc"
            [profiles.tv.audio]
            [profiles.tv.subtitles]
            [profiles.tv.output]
        "#;
        let err = scheduler_from(&Figment::new().merge(Toml::string(toml))).unwrap_err();
        assert!(matches!(err, ConfigError::UnknownProfile { .. }), "{err}");
    }

    #[test]
    fn empty_libraries_rejected() {
        let toml = r#"
            [profiles.tv.video]
            codec = "hevc"
            [profiles.tv.audio]
            [profiles.tv.subtitles]
            [profiles.tv.output]
        "#;
        let err = scheduler_from(&Figment::new().merge(Toml::string(toml))).unwrap_err();
        assert!(matches!(err, ConfigError::Empty("library")), "{err}");
    }

    #[test]
    fn worker_parses_durations_and_backends() {
        let toml = r#"
            concurrency = 2
            [verify]
            duration_tolerance = "2s"
            max_size_ratio = 1.2
            [[backend]]
            kind = "vaapi"
            device = "/dev/dri/renderD128"
            [[backend]]
            kind = "cpu"
        "#;
        let cfg = worker_from(&Figment::new().merge(Toml::string(toml))).unwrap();
        assert_eq!(cfg.concurrency, 2);
        assert_eq!(cfg.verify.duration_tolerance, Duration::from_secs(2));
        assert_eq!(cfg.backends.len(), 2);
        assert_eq!(cfg.backends[0].kind, BackendKind::Vaapi);
        assert_eq!(cfg.backends[1].kind, BackendKind::Cpu);
    }

    #[test]
    fn worker_rejects_zero_concurrency() {
        let toml = r#"
            concurrency = 0
            [[backend]]
            kind = "cpu"
        "#;
        let err = worker_from(&Figment::new().merge(Toml::string(toml))).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(_)), "{err}");
    }

    #[test]
    fn worker_requires_a_backend() {
        let err = worker_from(&Figment::new().merge(Toml::string("concurrency = 1"))).unwrap_err();
        assert!(matches!(err, ConfigError::Empty("backend")), "{err}");
    }

    #[test]
    fn relative_library_path_rejected() {
        let toml = r#"
            [[library]]
            name = "tv"
            path = "relative/tv"
            profile = "tv"
            [profiles.tv.video]
            codec = "hevc"
            [profiles.tv.audio]
            [profiles.tv.subtitles]
            [profiles.tv.output]
        "#;
        let err = scheduler_from(&Figment::new().merge(Toml::string(toml))).unwrap_err();
        assert!(matches!(err, ConfigError::RelativePath { .. }), "{err}");
    }

    #[test]
    fn duplicate_library_name_rejected() {
        let toml = r#"
            [[library]]
            name = "tv"
            path = "/hdd/tv"
            profile = "tv"
            [[library]]
            name = "tv"
            path = "/hdd/tv2"
            profile = "tv"
            [profiles.tv.video]
            codec = "hevc"
            [profiles.tv.audio]
            [profiles.tv.subtitles]
            [profiles.tv.output]
        "#;
        let err = scheduler_from(&Figment::new().merge(Toml::string(toml))).unwrap_err();
        assert!(matches!(err, ConfigError::DuplicateLibrary(_)), "{err}");
    }

    #[test]
    fn worker_rejects_nan_inf_and_small_size_ratio() {
        for bad in ["nan", "inf", "0.5", "-3.0"] {
            let toml = format!(
                "concurrency = 1\n[verify]\nmax_size_ratio = {bad}\n[[backend]]\nkind = \"cpu\"\n"
            );
            let err = worker_from(&Figment::new().merge(Toml::string(&toml))).unwrap_err();
            assert!(
                matches!(err, ConfigError::BadRatio(_)),
                "ratio {bad} should be rejected, got {err}"
            );
        }
    }

    // A scheduler config with one library + a `tv` profile carrying `rule_toml`.
    fn sched_with_rule(rule_toml: &str) -> Result<SchedulerConfig, ConfigError> {
        let toml = format!(
            r#"
            [[library]]
            name = "tv"
            path = "/hdd/tv"
            profile = "tv"
            [profiles.tv.video]
            codec = "hevc"
            [profiles.tv.audio]
            [profiles.tv.subtitles]
            [profiles.tv.output]
            container = "mkv"
            {rule_toml}
        "#
        );
        scheduler_from(&Figment::new().merge(Toml::string(&toml)))
    }

    #[test]
    fn good_rules_load() {
        let cfg = sched_with_rule(
            r#"
            [[profiles.tv.rule]]
            when = "video.height >= 2160"
            set = { "video.codec" = "av1", "video.quality.value" = "${video.height >= 2160 ? 24 : 22}" }
        "#,
        )
        .unwrap();
        assert_eq!(cfg.profiles["tv"].rules.len(), 1);
    }

    #[test]
    fn cel_syntax_error_fails_load() {
        let err = sched_with_rule("[[profiles.tv.rule]]\nwhen = \"video.height >=\"\nset = {}")
            .unwrap_err();
        assert!(matches!(err, ConfigError::InvalidRule { .. }), "{err}");
    }

    #[test]
    fn unknown_context_field_fails_load() {
        // `heigth` is not a CEL context field — canary evaluation surfaces it at load.
        let err = sched_with_rule(
            "[[profiles.tv.rule]]\nwhen = \"video.heigth >= 2160\"\nset = { \"video.codec\" = \"av1\" }",
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::InvalidRule { .. }), "{err}");
    }

    #[test]
    fn static_out_of_range_set_fails_load() {
        // A literal 99 on the 0..=51 quality.value is statically invalid → load error.
        let err = sched_with_rule(
            "[[profiles.tv.rule]]\nwhen = \"true\"\nset = { \"video.quality.value\" = 99 }",
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::InvalidRule { .. }), "{err}");
    }

    #[test]
    fn unknown_set_path_fails_load() {
        let err = sched_with_rule(
            "[[profiles.tv.rule]]\nwhen = \"true\"\nset = { \"video.bogus\" = \"x\" }",
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::InvalidRule { .. }), "{err}");
    }

    #[test]
    fn profile_typo_in_scheduler_is_rejected() {
        // deny_unknown_fields on the engine config structs: a typo'd profile key
        // fails loudly instead of silently falling back to a default.
        let toml = r#"
            [[library]]
            name = "tv"
            path = "/hdd/tv"
            profile = "tv"
            [profiles.tv.video]
            codec = "hevc"
            qualiy = 20
            [profiles.tv.audio]
            [profiles.tv.subtitles]
            [profiles.tv.output]
        "#;
        let err = scheduler_from(&Figment::new().merge(Toml::string(toml))).unwrap_err();
        assert!(matches!(err, ConfigError::Figment(_)), "{err}");
    }
}
