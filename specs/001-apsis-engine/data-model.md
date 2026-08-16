# Phase 1 — Data Model: apsis-engine

Types mirror the Python `_engine` and the design doc §4. Rust-flavored sketches; the
wire/serde types use `serde`.

## Inputs

### Probe / StreamInfo (from ffprobe JSON)
```rust
struct Probe { video: Option<StreamInfo>, audio: Vec<StreamInfo>, subtitles: Vec<StreamInfo>,
               container: Option<String> }
struct StreamInfo {
  index: u32, kind: StreamKind, codec: String, language: String,   // "und" if untagged
  channels: Option<u32>, title: Option<String>, is_default: bool,
  hdr: bool,                                                        // derived from color metadata
}
enum StreamKind { Video, Audio, Subtitle }
```

### Profile (from config; validated with `garde`)
```rust
struct Profile { video: VideoConfig, audio: AudioConfig, subtitles: SubtitleConfig, output: OutputConfig }
struct VideoConfig { codec: VideoCodec, quality: u8 /*0..=51*/, encoder: Encoder,
                     fallback: Fallback, skip_codecs: Vec<String>, hdr_policy: HdrPolicy }
struct AudioConfig { keep_languages: Vec<String>, default_language: String, priority: Vec<String>,
                     remove_commentary: bool, add_stereo: StereoConfig, preserve_surround: bool }
struct StereoConfig { codec: String, bitrate: u32, channels: u32, languages: Vec<String> }
struct SubtitleConfig { keep_languages: Vec<String>, default_language: String,
                        remove_formats: Vec<String>, remove_commentary: bool }
struct OutputConfig { container: String, replace_original: bool }
enum VideoCodec { Hevc, Av1 }   enum Encoder { Vaapi, Cpu }
enum Fallback { Cpu, None }     enum HdrPolicy { Copy, Tonemap, Encode }
```

## Output — FilePlan (the coordinator→worker contract)
```rust
struct FilePlan {
  video: VideoPlan,
  audio: Vec<AudioTrackPlan>,
  subtitles: Vec<SubtitleTrackPlan>,
  container: String,
  replace_original: bool,
  should_skip: bool,          // true → already compliant
  reasons: Vec<PlanReason>,   // why work is needed (for logs/observability)
}
struct VideoPlan { source_index: Option<u32>, source_codec: Option<String>,
                   target_codec: VideoCodec, action: VideoAction }
enum VideoAction { Copy, Encode, Unsupported }
struct AudioTrackPlan { source_index: u32, language: String, action: TrackAction,
                        target_codec: String, target_channels: u32, title: String, default: bool }
enum TrackAction { Copy, Encode, Drop, Generate }
struct SubtitleTrackPlan { source_index: u32, language: String, action: SubAction,
                           codec: String, title: Option<String>, default: bool }
enum SubAction { Copy, Drop }
struct PlanReason { code: String, scope: String, message: String }
```

## Backend materialization
```rust
trait Backend {
  fn kind(&self) -> BackendKind;
  fn build(&self, plan: &FilePlan, input: &Path, output: &Path) -> FfmpegCommandSpec;
}
enum BackendKind { Vaapi, Cpu /* Nvenc later */ }

struct FfmpegCommandSpec {
  inputs: Vec<PathBuf>,     // main file (+ optional CC sidecar, later)
  global: Vec<String>,      // -init_hw_device / -hwaccel / device
  maps: Vec<String>,        // per-output-stream: -map / -c:* / -metadata / -disposition
  output: PathBuf,          // temp on same FS as source (worker sets path)
}
impl FfmpegCommandSpec { fn to_args(&self) -> Vec<String>; }  // deterministic → golden-testable
```

## Errors
```rust
enum EngineError { Probe(String), ParseJson(serde_json::Error), Config(garde::Report),
                   NoVideoStream, Ffprobe(std::io::Error) }
```
All engine entry points return `Result<_, EngineError>`; no panics (FR-010).

## Relationships

- `Profile` + `Probe` → `plan(...)` → `FilePlan` (backend-neutral).
- `FilePlan` + `Backend` → `build(...)` → `FfmpegCommandSpec` → `to_args()` → the exact
  ffmpeg invocation.
- `should_skip = reasons.is_empty()` (compliant). The never-drop-all-audio rule
  (FR-006) is enforced inside audio planning before the plan is returned.
