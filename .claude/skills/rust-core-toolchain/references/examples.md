# Examples : Rust Toolchain in Practice

Working, verified snippets covering install, channel management, project setup, cross-compilation, lint configuration, MSRV, and CI. All examples target Rust 1.85+, edition 2024.

## 1. Install rustup and the stable toolchain

Linux / macOS :

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Follow prompts; choose "1" for default install.
source "$HOME/.cargo/env"
rustc --version
# expected: rustc 1.85.0 (or later) (xxxxxxxxx YYYY-MM-DD)
```

Windows :

Download and run `rustup-init.exe` from https://www.rust-lang.org/tools/install. After install, open a new shell :

```powershell
rustc --version
```

Source : https://www.rust-lang.org/tools/install.

## 2. Update everything

```bash
rustup update          # update all installed toolchains
rustup self update     # update rustup itself
rustup show            # show active toolchain + overrides
```

## 3. Switch channel temporarily

Without changing the global default :

```bash
# One command on nightly:
cargo +nightly build

# Or set a directory override:
rustup override set nightly
cargo build       # now uses nightly in this directory
rustup override unset
```

## 4. Install a date-pinned nightly

When a project requires a specific nightly (e.g. for `cargo-fuzz`) :

```bash
rustup toolchain install nightly-2025-05-15
rustup override set nightly-2025-05-15
```

Document the reason in `rust-toolchain.toml` or a project README. NEVER use bare `nightly` for code you intend to ship.

## 5. Project-pinned toolchain (`rust-toolchain.toml`)

In the project root, create :

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.85.0"
components = ["rustfmt", "clippy", "rust-analyzer"]
targets = ["wasm32-unknown-unknown"]
profile = "minimal"
```

Now any `cargo` / `rustc` invocation in this directory uses `1.85.0` with the listed components, auto-installing if missing.

## 6. Install a component

```bash
rustup component add rust-src
rustup component add miri --toolchain nightly
```

## 7. Cross-compile to a target

```bash
# Add target's std
rustup target add aarch64-apple-darwin

# Build
cargo build --release --target aarch64-apple-darwin

# Output is at target/aarch64-apple-darwin/release/<bin-name>
```

For non-trivial cross-compiles (Windows from Linux, embedded), configure the linker :

```toml
# .cargo/config.toml
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"

[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
```

## 8. New project with edition 2024

```bash
cargo new --bin my-app
cd my-app
cat Cargo.toml
```

Output (cargo 1.85+ defaults to edition 2024) :

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2024"

[dependencies]
```

Add an MSRV :

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
```

## 9. Workspace setup

```toml
# Cargo.toml at workspace root
[workspace]
resolver = "3"  # default in edition 2024
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
authors = ["Your Name <you@example.com>"]
license = "MIT"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
```

Inside a workspace member :

```toml
# crates/foo/Cargo.toml
[package]
name = "foo"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
```

## 10. Project-level lint config

```toml
# Cargo.toml
[lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"

[lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }
# Specific opts:
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
# Pedantic noise opts-out:
module_name_repetitions = "allow"
missing_errors_doc = "allow"
similar_names = "allow"
```

For workspaces, inherit :

```toml
# Workspace root Cargo.toml
[workspace.lints.clippy]
all = { level = "deny", priority = -1 }

# Member Cargo.toml
[lints]
workspace = true
```

## 11. Run clippy in CI

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

The `--all-targets` flag covers bins, libs, tests, examples, and benches. `-D warnings` turns every warning (including warn-by-default clippy categories) into a hard error.

For a multi-platform matrix, run for each tier-1 target :

```bash
cargo clippy --target x86_64-unknown-linux-gnu -- -D warnings
cargo clippy --target wasm32-unknown-unknown -- -D warnings
```

## 12. rustfmt config

```toml
# rustfmt.toml
edition = "2024"
max_width = 100
use_field_init_shorthand = true
match_arm_blocks = true
reorder_imports = true
reorder_modules = true
```

Run :

```bash
cargo fmt            # format
cargo fmt -- --check # check, exit non-zero if changes would be made (CI)
```

## 13. Custom release profile (size-optimized)

```toml
# Cargo.toml
[profile.release-small]
inherits = "release"
opt-level = "z"
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "abort"
```

Build :

```bash
cargo build --profile release-small
ls -lh target/release-small/my-app
```

`panic = "abort"` skips the unwinder, saving ~10-20 KB. Only use if you don't catch panics.

## 14. CI workflow (GitHub Actions)

```yaml
# .github/workflows/ci.yml
name: CI
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # rust-toolchain.toml in the repo pins the toolchain.
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets --all-features -- -D warnings
      - run: cargo test --all-features
```

When `rust-toolchain.toml` is present, `dtolnay/rust-toolchain@stable` respects it and installs the pinned version instead.

## 15. MSRV-aware dep resolution

`.cargo/config.toml` :

```toml
[resolver]
incompatible-rust-versions = "fallback"
```

Now when you add a dep :

```bash
cargo add serde
```

The resolver prefers the highest `serde` version whose `rust-version` is `<=` your workspace `rust-version`. Without this setting, cargo picks the absolute highest semver and may pull in a dep that requires a newer rustc.

Source : https://blog.rust-lang.org/2025/01/09/Rust-1.84.0/.

## 16. Generate API docs

```bash
cargo doc --no-deps --open       # crate's own docs
cargo doc --document-private-items # include private items
RUSTDOCFLAGS="--cfg docsrs" cargo +nightly doc --no-deps  # for docs.rs preview
```

## 17. Inspect macro expansions

```bash
cargo install cargo-expand
cargo expand
cargo expand --bin my-app
cargo expand --test my_test mod::path
```

Useful for debugging macro_rules! and derive macros. Output is post-expansion Rust source.

## 18. Profile a release build

Generate optimized debug info :

```toml
# Cargo.toml
[profile.profiling]
inherits = "release"
debug = true
```

```bash
cargo build --profile profiling
perf record --call-graph=dwarf ./target/profiling/my-app
perf report
```

## 19. Bench a function

```rust
// benches/my_bench.rs (requires criterion = "0.5" as a dev-dep)
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn fib(n: u64) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => fib(n - 1) + fib(n - 2),
    }
}

fn bench_fib(c: &mut Criterion) {
    c.bench_function("fib 20", |b| b.iter(|| fib(black_box(20))));
}

criterion_group!(benches, bench_fib);
criterion_main!(benches);
```

In Cargo.toml :

```toml
[dev-dependencies]
criterion = "0.5"

[[bench]]
name = "my_bench"
harness = false
```

Run : `cargo bench --bench my_bench`.

## 20. Update rust-toolchain.toml after a rustc release

```bash
# Update the pin
sed -i 's/channel = "1.85.0"/channel = "1.87.0"/' rust-toolchain.toml

# Apply
rustup update
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

If new clippy warnings appear, fix them or explicitly `#[allow]` with a comment justifying the exception. NEVER silently widen `#[allow]` scopes.
