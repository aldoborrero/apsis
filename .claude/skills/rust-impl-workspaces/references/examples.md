# rust-impl-workspaces: full examples

Real, compilable workspace layouts. Each example shows the **complete** set of manifests, file tree, and the commands that drive them.

---

## Example 1: virtual workspace with three members

The canonical layout: one `core` library, one `cli` binary, one `server` binary, all sharing dependencies.

### File tree

```
my-app/
├── Cargo.toml                  <- workspace root (virtual)
├── Cargo.lock                  <- single lockfile
├── rust-toolchain.toml
├── target/                     <- single build output
└── crates/
    ├── core/
    │   ├── Cargo.toml
    │   └── src/lib.rs
    ├── cli/
    │   ├── Cargo.toml
    │   └── src/main.rs
    └── server/
        ├── Cargo.toml
        └── src/main.rs
```

### Root `Cargo.toml`

```toml
[workspace]
resolver = "3"
members  = ["crates/*"]
default-members = ["crates/cli", "crates/server"]

[workspace.package]
edition      = "2024"
rust-version = "1.85"
license      = "MIT OR Apache-2.0"
authors      = ["My Team <team@example.com>"]
repository   = "https://github.com/example/my-app"

[workspace.dependencies]
# Shared runtime
tokio     = { version = "1", features = ["rt-multi-thread", "macros", "signal"] }
# Shared serialization
serde     = { version = "1", features = ["derive"] }
serde_json = "1"
# Shared error handling
anyhow    = "1"
thiserror = "2"
# Shared logging
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
# Internal
core = { path = "crates/core" }

[workspace.lints.rust]
unsafe_code = "forbid"
unused_imports = "warn"

[workspace.lints.clippy]
unwrap_used = "warn"
needless_pass_by_value = "warn"
```

### `crates/core/Cargo.toml`

```toml
[package]
name    = "core"
version = "0.1.0"
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
authors.workspace      = true
repository.workspace   = true

[dependencies]
serde     = { workspace = true }
thiserror = { workspace = true }
tracing   = { workspace = true }

[lints]
workspace = true
```

### `crates/cli/Cargo.toml`

```toml
[package]
name    = "cli"
version = "0.1.0"
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
authors.workspace      = true
repository.workspace   = true

[dependencies]
core    = { workspace = true }
tokio   = { workspace = true, features = ["fs", "process"] }
serde   = { workspace = true }
anyhow  = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }

[dev-dependencies]
tempfile = "3"

[lints]
workspace = true
```

### `crates/server/Cargo.toml`

```toml
[package]
name    = "server"
version = "0.1.0"
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
authors.workspace      = true
repository.workspace   = true

[dependencies]
core    = { workspace = true }
tokio   = { workspace = true, features = ["net"] }
serde   = { workspace = true }
serde_json = { workspace = true }
anyhow  = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }

[lints]
workspace = true
```

### Operating the workspace

```bash
# Build only the default-members (cli + server, the user-facing binaries)
cargo build

# Build everything including core
cargo build --workspace

# Test every member
cargo test --workspace

# Test just core
cargo test -p core

# Run the cli
cargo run -p cli -- some-arg

# Skip a known-flaky integration crate (assume legacy/ existed)
cargo test --workspace --exclude legacy

# Show the full dependency tree
cargo tree --workspace

# Update only serde across the entire workspace
cargo update -p serde
```

---

## Example 2: root-package workspace (one main crate plus helpers)

When the project IS one main crate (a published library), and the helpers are thin support packages, the root manifest can be both `[package]` AND `[workspace]`.

### File tree

```
my-lib/
├── Cargo.toml         <- both [package] and [workspace]
├── src/lib.rs
└── helpers/
    ├── codegen/
    │   ├── Cargo.toml
    │   └── src/main.rs
    └── fixtures/
        ├── Cargo.toml
        └── src/lib.rs
```

### Root `Cargo.toml`

```toml
[package]
name         = "my-lib"
version      = "0.4.2"
edition      = "2024"
rust-version = "1.85"
license      = "MIT OR Apache-2.0"
description  = "A library for X"

[workspace]
resolver = "3"
members  = ["helpers/*"]

[workspace.dependencies]
my-lib = { path = "." }
serde  = "1"

[dependencies]
serde = { workspace = true }
```

### `helpers/codegen/Cargo.toml`

```toml
[package]
name    = "codegen"
version = "0.0.0"
edition = "2024"
publish = false           # internal tool, never released

[dependencies]
my-lib = { workspace = true }
```

Notes:

- The root `[package]` IS the published library; the `helpers/` are unpublished tools (`publish = false`).
- `resolver = "3"` here lives in `[workspace]` because that table exists; if the workspace were absent it would live in `[package]`.

ALWAYS pick this layout only when the published-crate AND the workspace-root genuinely are the same thing. Otherwise the virtual layout is cleaner.

---

## Example 3: monorepo with `crates/*` glob and explicit exclude

A repo with many crates auto-generated by `cargo new`, but with one legacy crate that no longer compiles and one scratch dir for experiments.

### Root `Cargo.toml`

```toml
[workspace]
resolver = "3"
members  = ["crates/*"]
exclude  = [
    "crates/legacy",    # superseded by crates/core, kept for reference
    "crates/scratch",   # experimental, not part of build
]

[workspace.package]
edition      = "2024"
rust-version = "1.85"
license      = "Apache-2.0"

[workspace.dependencies]
clap   = { version = "4", features = ["derive"] }
serde  = { version = "1", features = ["derive"] }
tokio  = { version = "1", features = ["full"] }

[workspace.lints.rust]
unsafe_code = "forbid"
```

Adding a new crate is a single command (no manifest change needed because of the glob):

```bash
cargo new --lib crates/new-feature
# new-feature is automatically a workspace member
```

---

## Example 4: a member opts out of workspace lints

The workspace forbids `unsafe_code`, but `crates/ffi-bridge` legitimately needs `unsafe`. Drop the inheritance line and write a member-level `[lints]` table.

### Root `Cargo.toml` (excerpt)

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
```

### `crates/ffi-bridge/Cargo.toml`

```toml
[package]
name = "ffi-bridge"
version = "0.1.0"
edition.workspace = true

# Note: no [lints] workspace = true here. We define our own.
[lints.rust]
unsafe_code = "allow"
unused_imports = "warn"

[lints.clippy]
unwrap_used = "warn"
```

ALWAYS comment why a member opts out of the workspace lints (a single `# ffi requires unsafe extern blocks` line is enough).

---

## Example 5: feature additions at the call site

The workspace declares the minimum useful feature set; one member needs more.

### Root `Cargo.toml`

```toml
[workspace.dependencies]
tokio = { version = "1", features = ["rt", "macros"] }
```

### `crates/web-server/Cargo.toml`

```toml
[dependencies]
tokio = { workspace = true, features = ["net", "fs", "signal"] }
```

The effective feature set for `web-server` is the **union**: `rt`, `macros`, `net`, `fs`, `signal`. Because Cargo features are additive, this never breaks members that did not request the extras.

NEVER subtract features at the call site. There is no syntax for "I want `tokio` but without `macros`". If a member must not have a feature, that feature should never have been in the workspace declaration.

---

## Example 6: release profile at workspace root only

Profile overrides MUST live at the root. Putting them on a member is silently ignored.

### Root `Cargo.toml`

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "abort"

[profile.dev]
opt-level = 0
debug = true

[profile.release-with-debug]
inherits = "release"
debug = "line-tables-only"
strip = "none"
```

Then:

```bash
cargo build --release                     # uses [profile.release]
cargo build --profile release-with-debug  # custom profile
```

---

## Example 7: `default-members` for selective CI

A large workspace has 20 crates. Only 4 are user-facing; the other 16 are internal libs that get built transitively. CI's smoke job uses `cargo build` to compile just the 4 entry points (transitively building everything they need).

### Root `Cargo.toml` (excerpt)

```toml
[workspace]
resolver = "3"
members  = ["crates/*"]
default-members = [
    "crates/cli",
    "crates/server",
    "crates/migration-tool",
    "crates/admin-ui",
]
```

```bash
# CI smoke job (fast):
cargo build

# CI full test job:
cargo test --workspace --all-features
```
