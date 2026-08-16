# Methods : Rust Toolchain Surface

Complete reference for `rustup`, `cargo`, `rustc`, `clippy`, `rustfmt`. All claims verified against the official rustup, cargo, rustc, clippy, and rustfmt books on 2026-05-19. See SKILL.md for quick-reference decision trees.

## 1. rustup

### 1.1 Install / self-management

| Command | Effect |
|---|---|
| `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` | Install rustup + default-host stable toolchain |
| `rustup self update` | Update rustup itself (not the toolchain) |
| `rustup update` | Update all installed toolchains |
| `rustup update stable` | Update only the stable toolchain |
| `rustup self uninstall` | Remove rustup and all toolchains |
| `rustup show` | Print active toolchain, override source, installed toolchains |
| `rustup which rustc` | Print the rustc binary path the current directory resolves to |

Source : https://rust-lang.github.io/rustup/.

### 1.2 Channels and toolchains

The toolchain naming grammar is `<channel>[-<date>][-<host>]` :

- `stable`
- `beta`
- `nightly`
- `nightly-2025-05-15` (date-pinned nightly)
- `1.85.0` (version-pinned stable)
- `stable-x86_64-pc-windows-msvc` (host-pinned)

Commands :

| Command | Effect |
|---|---|
| `rustup toolchain list` | List installed toolchains |
| `rustup toolchain install nightly` | Install a toolchain |
| `rustup toolchain install nightly-2025-05-15` | Install a date-pinned nightly |
| `rustup toolchain install 1.85.0` | Install a specific stable version |
| `rustup toolchain uninstall nightly-2024-06-01` | Remove a toolchain |
| `rustup default stable` | Set the global default toolchain |
| `rustup default 1.85.0` | Pin global default to a version |
| `rustup override set nightly` | Pin the current directory to nightly |
| `rustup override unset` | Remove the directory override |

Source : https://rust-lang.github.io/rustup/concepts/toolchains.html.

### 1.3 Components

Components are pieces of a toolchain that can be installed independently :

| Component | Default on stable | Purpose |
|---|---|---|
| `rustc` | yes | Compiler |
| `cargo` | yes | Build/package tool |
| `rust-std` | yes (host) | Pre-built standard library |
| `rustfmt` | yes | Formatter |
| `clippy` | yes | Linter |
| `rust-analyzer` | yes (since 1.65) | LSP server |
| `rust-src` | no | Standard library source code |
| `rust-docs` | yes | Local API docs |
| `miri` | no (nightly only) | UB interpreter |
| `llvm-tools` | no | objcopy, profdata, cov |
| `rustc-dev` | no | Compiler crates (for tool authors) |
| `rust-analysis` | no | Type analysis output |

Commands :

| Command | Effect |
|---|---|
| `rustup component list` | List available components for active toolchain |
| `rustup component list --installed` | List installed components |
| `rustup component add clippy` | Install a component |
| `rustup component add rust-src --toolchain nightly` | Install in a specific toolchain |
| `rustup component remove miri` | Remove a component |

Source : https://rust-lang.github.io/rustup/concepts/components.html.

### 1.4 Targets (cross-compilation)

Target triples have the shape `<arch>-<vendor>-<sys>-<abi>`. Common examples :

| Triple | Use |
|---|---|
| `x86_64-unknown-linux-gnu` | Linux desktop/server |
| `x86_64-unknown-linux-musl` | Static Linux binary |
| `aarch64-unknown-linux-gnu` | ARM64 Linux |
| `aarch64-apple-darwin` | Apple Silicon macOS |
| `x86_64-pc-windows-msvc` | Windows native |
| `x86_64-pc-windows-gnu` | Windows via MinGW |
| `wasm32-unknown-unknown` | WebAssembly without WASI |
| `wasm32-wasip1` | WASI preview 1 |
| `thumbv7em-none-eabihf` | ARM Cortex-M4F (embedded) |

Targets have a support tier : tier 1 (guaranteed to build and test), tier 2 (guaranteed to build), tier 3 (best-effort). Source : https://doc.rust-lang.org/rustc/platform-support.html.

Commands :

| Command | Effect |
|---|---|
| `rustup target list` | List available targets |
| `rustup target list --installed` | List installed targets |
| `rustup target add wasm32-unknown-unknown` | Install a target's std |
| `rustup target add aarch64-apple-darwin --toolchain stable` | Add to specific toolchain |
| `rustup target remove wasm32-unknown-unknown` | Remove a target |

### 1.5 `rust-toolchain.toml`

Per-project override file in the project root :

```toml
[toolchain]
channel = "1.85.0"
components = ["rustfmt", "clippy", "rust-analyzer"]
targets = ["wasm32-unknown-unknown", "aarch64-apple-darwin"]
profile = "minimal"
```

Fields :

- `channel` : either a channel name (`stable`/`beta`/`nightly`), a date-pinned nightly, or a version (`1.85.0`).
- `components` : extra components to install on top of `profile`.
- `targets` : extra targets to install.
- `profile` : `minimal` (just rustc + cargo + rust-std), `default` (adds rustfmt + clippy + docs), `complete` (everything available).

Resolution order : `rust-toolchain.toml` > `rust-toolchain` (legacy text file) > directory override > default toolchain.

Source : https://rust-lang.github.io/rustup/overrides.html.

### 1.6 Profile shortcuts

| Profile | Components |
|---|---|
| `minimal` | rustc + cargo + rust-std |
| `default` | minimal + rust-docs + rustfmt + clippy |
| `complete` | All available components (NEVER recommended on stable, useful only for testing) |

Set via `rustup set profile default`. Source : https://rust-lang.github.io/rustup/concepts/profiles.html.

## 2. cargo

### 2.1 Project lifecycle commands

| Command | Effect | Notes |
|---|---|---|
| `cargo new <name>` | New binary crate | Add `--lib` for library, `--vcs none` to skip git init |
| `cargo init` | Initialize current dir as crate | Useful for existing directories |
| `cargo build` | Compile in `dev` profile | Output in `target/debug/` |
| `cargo build --release` | Compile in `release` profile | Output in `target/release/`, optimized |
| `cargo run` | Compile + run default binary | Pass args after `--` : `cargo run -- --flag` |
| `cargo run --bin name` | Run specific binary in multi-bin crate | |
| `cargo check` | Type-check WITHOUT codegen | Fastest dev feedback |
| `cargo clean` | Remove `target/` | |
| `cargo test` | Build + run unit + integration tests | Includes doctests |
| `cargo test --release` | Run tests in release profile | Slower compile, faster run |
| `cargo bench` | Run benchmarks | Stable `--bench` targets; libtest harness still nightly-only |
| `cargo doc` | Build API docs into `target/doc/` | Use `--open` to launch browser |

### 2.2 Dependency commands

| Command | Effect |
|---|---|
| `cargo add <crate>` | Add a runtime dep to Cargo.toml |
| `cargo add <crate> --dev` | Add a dev dep |
| `cargo add <crate> --build` | Add a build-script dep |
| `cargo add <crate>@1.2` | Pin to a major.minor |
| `cargo add <crate> --features f1,f2` | Enable features |
| `cargo remove <crate>` | Remove a dep |
| `cargo update` | Update Cargo.lock within semver constraints |
| `cargo update -p <crate>` | Update only one crate |
| `cargo tree` | Print dep tree |
| `cargo tree -i <crate>` | Print inverse tree (who depends on <crate>) |
| `cargo tree -d` | Highlight duplicate dependencies |
| `cargo fetch` | Download all deps to local cache without building |
| `cargo vendor` | Copy all deps to `vendor/` for offline build |

`cargo add` / `cargo remove` were stabilized in Rust 1.62 / 1.66. Source : https://doc.rust-lang.org/cargo/commands/cargo-add.html.

### 2.3 Quality commands

| Command | Effect |
|---|---|
| `cargo fmt` | Format the current crate |
| `cargo fmt --all` | Format every member of the workspace |
| `cargo fmt -- --check` | Fail if formatting differs (CI) |
| `cargo clippy` | Run clippy lints |
| `cargo clippy --all-targets --all-features` | Lint every target and feature combo |
| `cargo clippy -- -D warnings` | Promote all warnings to errors |
| `cargo clippy --fix` | Auto-apply clippy suggestions |
| `cargo fix --edition` | Apply lints to migrate to next edition |
| `cargo audit` | Check `Cargo.lock` against advisory DB (requires `cargo install cargo-audit`) |
| `cargo deny check` | Check licenses, bans, advisories (requires `cargo install cargo-deny`) |

### 2.4 Publication commands

| Command | Effect |
|---|---|
| `cargo login <token>` | Save crates.io token |
| `cargo publish --dry-run` | Build + check publishability without uploading |
| `cargo publish` | Upload to crates.io |
| `cargo yank --version 0.1.0` | Yank a version (prevents new uses) |
| `cargo owner --add github:org:team` | Manage crate owners |

### 2.5 Cargo profiles (Cargo.toml `[profile.*]`)

Default profiles :

| Profile | `opt-level` | `debug` | `lto` | `codegen-units` | `incremental` |
|---|---|---|---|---|---|
| `dev` | 0 | true | false | 256 | true |
| `release` | 3 | false | false | 16 | false |
| `test` (inherits dev) | 0 | true | false | 256 | true |
| `bench` (inherits release) | 3 | false | false | 16 | false |

Custom profile :

```toml
[profile.release-lto]
inherits = "release"
lto = "fat"
codegen-units = 1
strip = "symbols"
```

Invoke : `cargo build --profile release-lto`. Source : https://doc.rust-lang.org/cargo/reference/profiles.html.

### 2.6 `[lints]` table (Rust 1.74+)

```toml
[lints.rust]
unused_imports = "warn"
unsafe_code = "forbid"

[lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
```

Levels : `allow`, `warn`, `deny`, `forbid`. Priority defaults to 0; lower-priority groups can be selectively overridden by individual lints with higher priority. Source : https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section.

## 3. rustc

`cargo` invokes `rustc` per crate. Direct `rustc` use is rare; tune via cargo profiles or `RUSTFLAGS`.

### 3.1 Edition and target flags

| Flag | Effect |
|---|---|
| `--edition 2024` | Compile with edition 2024 rules (also set via Cargo.toml) |
| `--target <triple>` | Cross-compile to a target |
| `--crate-type lib`, `bin`, `dylib`, `cdylib`, `staticlib`, `rlib`, `proc-macro` | Output type |
| `--emit asm,llvm-ir,obj` | Emit intermediate representations |

### 3.2 Codegen flags via `-C`

| Flag | Values | Meaning |
|---|---|---|
| `-C opt-level=N` | `0`, `1`, `2`, `3`, `s`, `z` | Optimization level. `s` = small, `z` = smallest |
| `-C debuginfo=N` | `0`, `1`, `2` (or `none`/`limited`/`full`) | Debug info level |
| `-C lto=BOOL` | `false`, `true`, `thin`, `fat`, `off` | Link-time optimization |
| `-C codegen-units=N` | integer | Parallel codegen units. 1 = best opt, slowest |
| `-C panic=MODE` | `unwind`, `abort` | Panic strategy |
| `-C target-cpu=NAME` | `native`, `x86-64-v3`, `apple-m1`, ... | CPU-specific codegen |
| `-C strip=MODE` | `none`, `debuginfo`, `symbols` | Strip symbols from binary |
| `-C overflow-checks=BOOL` | `true`/`false` | Insert integer overflow checks |
| `-C link-arg=ARG` | linker flag | Forward arg to linker |

Set in Cargo.toml profile :

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "symbols"
panic = "abort"
```

Source : https://doc.rust-lang.org/rustc/codegen-options/index.html.

### 3.3 `RUSTFLAGS` environment variable

Apply flags to all invocations :

```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

ALWAYS prefer Cargo profiles for project-stable flags; reserve `RUSTFLAGS` for one-off or environment-specific tweaks.

### 3.4 Lint flags

| Flag | Effect |
|---|---|
| `-W <lint>` | Warn level |
| `-D <lint>` | Deny level (error) |
| `-F <lint>` | Forbid (cannot be unforbidden) |
| `-A <lint>` | Allow level |
| `--cap-lints LEVEL` | Cap all lints to a max level (used for dep builds) |

## 4. clippy

### 4.1 Categories

| Category | Default level | Total lints (approx 1.85) | Description |
|---|---|---|---|
| `correctness` | DENY | ~80 | Code that is objectively wrong. |
| `suspicious` | warn | ~30 | Patterns probably not intended. |
| `style` | warn | ~150 | Readability without functional impact. |
| `complexity` | warn | ~90 | Convoluted code. |
| `perf` | warn | ~50 | Inefficient patterns with a better form. |
| `pedantic` | allow | ~100 | Stricter quality, opt-in. |
| `restriction` | allow | ~100 | Opt-in coding constraints (e.g. ban `unwrap`). |
| `nursery` | allow | ~50 | Under development. |
| `cargo` | allow | ~10 | Cargo.toml metadata lints. |

Group meta : `clippy::all` = correctness + suspicious + style + complexity + perf. Pedantic / restriction / nursery / cargo are explicit opt-in. Source : https://rust-lang.github.io/rust-clippy/master/.

### 4.2 Configuration mechanisms

| Mechanism | Scope |
|---|---|
| `Cargo.toml [lints.clippy]` (preferred, 1.74+) | Project or workspace member |
| `#![warn(clippy::pedantic)]` at crate root | Crate-level |
| `#[allow(clippy::lint_name)]` on item | Single item |
| `clippy.toml` (config file) | Configurable lint thresholds |

`clippy.toml` configures lint-specific values :

```toml
# clippy.toml
msrv = "1.85"
cognitive-complexity-threshold = 30
type-complexity-threshold = 250
```

### 4.3 Common lint config patterns

```toml
# Cargo.toml
[lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
# Explicit opt-out for noisy pedantic lints
module_name_repetitions = "allow"
missing_errors_doc = "allow"
# Specific restriction lints
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
```

The `priority = -1` ensures group settings are evaluated before individual lints. Without it, the order is undefined.

## 5. rustfmt

### 5.1 Invocation

| Command | Effect |
|---|---|
| `cargo fmt` | Format current crate |
| `cargo fmt --all` | Format all workspace members |
| `cargo fmt -- --check` | Exit non-zero if files would change (CI) |
| `cargo fmt -- --emit files` | Write changes to disk (default) |
| `cargo fmt -- --emit stdout` | Print to stdout instead |
| `rustfmt path/file.rs` | Direct invocation on a file |

### 5.2 `rustfmt.toml` (stable options)

```toml
edition = "2024"
max_width = 100
tab_spaces = 4
hard_tabs = false
newline_style = "Auto"
use_field_init_shorthand = false
use_try_shorthand = false
remove_nested_parens = true
match_arm_blocks = true
reorder_imports = true
reorder_modules = true
```

### 5.3 Unstable options (nightly only)

Some commonly-requested options require nightly :

- `imports_granularity = "Crate"` (group imports by crate)
- `group_imports = "StdExternalCrate"`
- `format_strings = true`
- `wrap_comments = true`
- `comment_width = 80`

Invoke `cargo +nightly fmt` to apply them. Source : https://rust-lang.github.io/rustfmt/.

### 5.4 Per-item `#[rustfmt::skip]`

```rust
#[rustfmt::skip]
let aligned_matrix = [
    1, 0, 0,
    0, 1, 0,
    0, 0, 1,
];
```

Use sparingly; ALWAYS prefer adjusting `rustfmt.toml` over per-item skips.

## 6. MSRV management

### 6.1 Declaration

```toml
[package]
name = "mycrate"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
```

`cargo build` refuses to compile with `rustc` older than `rust-version`. Documented in `cargo` UI.

### 6.2 MSRV-aware resolver (1.84+)

Opt-in to a dep-version selection algorithm that respects MSRV :

```toml
# .cargo/config.toml
[resolver]
incompatible-rust-versions = "fallback"
```

`fallback` makes the resolver prefer dep versions compatible with the workspace MSRV when multiple satisfy semver. Set to `"allow"` (default) to disable. Source : https://blog.rust-lang.org/2025/01/09/Rust-1.84.0/.

### 6.3 MSRV policy implications

- Bumping MSRV is a SemVer minor bump for libraries, by convention.
- Dependencies that bump MSRV may force you to bump yours.
- `cargo msrv` (community tool, `cargo install cargo-msrv`) finds the minimum working version for a crate.

Source : https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field.
