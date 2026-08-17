# Quickstart — apsis-engine

## Build & test

```bash
cargo build -p apsis-engine
cargo test  -p apsis-engine                 # oracle + golden-command tests
cargo test  -p apsis-engine --features probe-exec   # + the ffprobe helper
```

## Use (as a library)

```rust
use apsis_engine::{Backend, HardwareConfig, VaapiBackend, parse_probe, plan};

let probe     = parse_probe(&ffprobe_json)?;          // or probe_file(path, ffprobe)
let file_plan = plan(input_path, &probe, &profile);   // profile from scheduler.toml
if file_plan.should_skip {
    // already compliant (or unsupported) → no job
} else {
    let backend = VaapiBackend {
        hardware: HardwareConfig::default(),          // hw_decode set, sei/async_depth, env
        vaapi_device: "/dev/dri/renderD128".into(),
        ffmpeg_path: "ffmpeg".into(),
    };
    let args = backend.build(&file_plan, &profile)?.build(); // exact ffmpeg argv for the worker
}
```

The command builder needs both the plan and the `Profile` (encoder, quality and stereo
bitrate live on the profile, mirroring the Python `command.py`). A `CpuBackend { ffmpeg_path }`
is the fallback: `libx265`/`libsvtav1` with `crf` + `preset`.

## Test strategy (maps to Success Criteria)

- **Oracle parity (SC-001)** — `tests/plan_oracle.rs`: for each fixture in
  `tests/fixtures/` (ffprobe JSON + the expected `FilePlan` captured from the Python
  `_engine`), assert `plan(&probe, &profile)` equals the expected plan.
- **Golden commands (SC-002)** — `tests/golden_cmd.rs`: for a set of (fixture, profile,
  backend), assert `backend.build(...).to_args()` equals the expected arg vector
  (including `-sei hdr` for VAAPI hevc/h264, and the CPU fallback command).
- **Skip correctness (SC-003)** — assert `should_skip == true` for every compliant
  fixture.
- **Standalone (SC-004)** — `cargo build -p apsis-engine` with no orchestration deps in
  `Cargo.toml`.

## Porting the oracle fixtures

From the Python `_engine` (branch `feat/unmanic-integration`), export for each existing
test case: the ffprobe JSON input and the resulting plan/command. Drop them into
`tests/fixtures/<case>/{probe.json, plan.json, cmd-vaapi.txt, cmd-cpu.txt}`. The Rust
tests iterate the fixtures directory — adding a case = adding a folder.
