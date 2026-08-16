---
name: rust-core-toolchain
description: >
  Use when the user asks about the Rust toolchain: rustup (channels, components, targets), cargo subcommands, rustc flags, clippy lint categories, rustfmt configuration, MSRV management, or how to set up a Rust workspace.
  Prevents using the wrong channel, missing required components, ignoring clippy categories, or making code unbuildable on the project's MSRV.
  Covers: rustup (channels stable/beta/nightly, components rustfmt/clippy/rust-src/rust-analyzer, `rustup target add`, `rust-toolchain.toml` override), cargo (most-used subcommands), rustc (edition flag, target triple, optimization levels), clippy (7 lint categories), rustfmt (edition-aware), MSRV management via `rust-version`.
  Keywords: rustup, cargo, rustc, clippy, rustfmt, toolchain, "install Rust", channel, stable, beta, nightly, "what command", component, target triple, "rust-toolchain.toml", optimization level, lint categories, correctness, pedantic, "how do I lint", "format Rust", MSRV, rust-version, "cargo build", "cargo check", "cargo test".
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# rust-core-toolchain

Mechanics of the Rust toolchain : `rustup` (toolchain installer/manager), `cargo` (build/package orchestrator), `rustc` (compiler), `clippy` (linter), `rustfmt` (formatter). This skill is the operational complement to `[[rust-core-language-versions]]` (which covers editions, channels-as-concept, MSRV-as-policy). For Cargo.toml mechanics see `[[rust-impl-cargo-project]]`. For edition-2024 language-level changes see `[[rust-syntax-edition-2024]]`.

## Quick Reference

### Tool boundaries

| Tool | Role | Owns |
|---|---|---|
| `rustup` | Toolchain manager | Channels, components, targets, version pinning |
| `cargo` | Build/package orchestrator | Builds, dependencies, tests, publishing, lints config |
| `rustc` | Compiler | Source-to-binary, codegen flags, edition flag |
| `clippy` | Linter (cargo subcommand `cargo clippy`) | Lint categories, additional checks beyond rustc |
| `rustfmt` | Formatter (cargo subcommand `cargo fmt`) | Whitespace, layout, edition-aware syntax shape |

ALWAYS invoke clippy and rustfmt via their cargo subcommands (`cargo clippy`, `cargo fmt`), not as standalone binaries.

### Install in one line

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

This installs `rustup`, which then installs `rustc`, `cargo`, `rustfmt`, and `clippy` on the default `stable` channel. Source : https://www.rust-lang.org/tools/install.

### Channel matrix

| Channel | Release cadence | Stability | Use when |
|---|---|---|---|
| `stable` | 6 weeks | API-stable | ALWAYS for production code |
| `beta` | 6 weeks | Pre-release of next stable | Verifying your crate before the next stable hits |
| `nightly` | Daily | Unstable features behind `#![feature(...)]` | A required feature is `#![feature(...)]`-gated |

NEVER ship production code on nightly without an explicit, documented reason and a pinned nightly date.

### Component matrix

| Component | Default on stable | Purpose |
|---|---|---|
| `rustc` | yes | Compiler |
| `cargo` | yes | Build/package tool |
| `rust-std` | yes (host target) | Pre-built standard library for the host |
| `rustfmt` | yes | Code formatter |
| `clippy` | yes | Linter |
| `rust-analyzer` | yes (since 1.65) | LSP for IDEs |
| `rust-src` | no (opt-in) | Standard library source for IDE go-to-definition |
| `miri` | no (nightly only) | UB interpreter for tests |
| `llvm-tools` | no (opt-in) | objcopy, profdata, cov (coverage, profiling) |
| `rustc-dev` | no (opt-in) | Compiler internals for tool authors |

Install : `rustup component add <name>`. Source : https://rust-lang.github.io/rustup/concepts/components.html.

### Cargo subcommand cheat sheet

| Subcommand | Use when |
|---|---|
| `cargo new <name>` | New binary crate (use `--lib` for library) |
| `cargo build` | Compile (dev profile by default) |
| `cargo build --release` | Compile with optimizations (release profile) |
| `cargo run` | Compile + run binary target |
| `cargo check` | Type-check WITHOUT codegen, fastest feedback loop |
| `cargo test` | Build + run unit + integration tests |
| `cargo clippy` | Lint, optional `-- -D warnings` to fail on any warning |
| `cargo fmt` | Format the current crate or workspace |
| `cargo doc --open` | Build and open API docs |
| `cargo update` | Update Cargo.lock to latest semver-compatible versions |
| `cargo tree` | Print dependency tree |
| `cargo expand` | Show macro expansions (requires `cargo install cargo-expand`) |
| `cargo install <name>` | Install a binary crate to `~/.cargo/bin` |
| `cargo publish` | Upload to crates.io |
| `cargo bench` | Run benchmarks (stable since 1.62 for `--bench` targets; libtest harness is nightly) |
| `cargo fetch` | Download deps without building |
| `cargo vendor` | Vendor all deps into `vendor/` for offline builds |

Source : https://doc.rust-lang.org/cargo/commands/index.html.

### Clippy lint categories (the seven plus two)

| Category | Default level | When to use |
|---|---|---|
| `correctness` | DENY | Code that is objectively wrong. NEVER `#[allow]` without justification. |
| `suspicious` | warn | Patterns likely unintended. Investigate every hit. |
| `style` | warn | Readability without functional impact. |
| `complexity` | warn | Unnecessarily convoluted code. |
| `perf` | warn | Inefficient patterns with a clear better form. |
| `pedantic` | allow | Stricter quality. Opt in for libraries. |
| `restriction` | allow | Opt-in coding constraints (e.g. forbid `unwrap()`). Pick individually, NEVER the whole group. |
| `nursery` | allow | Lints under development. Expect breakage. |
| `cargo` | allow | Cargo.toml metadata lints. |

The `clippy::all` group covers correctness + suspicious + style + complexity + perf. Pedantic / restriction / nursery / cargo are explicit opt-in. Source : https://rust-lang.github.io/rust-clippy/master/.

### `rust-toolchain.toml` (per-project override)

ALWAYS commit a `rust-toolchain.toml` for any project where CI and contributor toolchain must match.

```toml
[toolchain]
channel = "1.85.0"
components = ["rustfmt", "clippy", "rust-analyzer"]
targets = ["wasm32-unknown-unknown"]
profile = "minimal"
```

When `rustup` enters a directory containing this file, it auto-installs and uses the pinned toolchain. Source : https://rust-lang.github.io/rustup/overrides.html.

## Decision Trees

### "Which cargo command should I run?"

```
Did I change Cargo.toml?
├── YES → `cargo update` (lockfile) or `cargo build` (also resolves)
└── NO  → Did I change source?
         ├── Type-error iteration  → `cargo check` (fastest)
         ├── Need to run it        → `cargo run`
         ├── Need to test it       → `cargo test`
         ├── About to commit       → `cargo fmt && cargo clippy -- -D warnings && cargo test`
         └── About to publish      → `cargo publish --dry-run` first
```

### "Which channel do I install?"

```
Does the project need a `#![feature(...)]` gate?
├── YES → nightly. Pin via `channel = "nightly-YYYY-MM-DD"` in rust-toolchain.toml + DOCUMENT WHY.
└── NO  → Are you on the release-train one cycle ahead?
         ├── YES (testing pre-release) → beta
         └── NO  → ALWAYS stable.
```

### "Which clippy categories do I enable?"

```
Project type?
├── Application binary → clippy::all + perf + suspicious (the default warn set)
├── Library            → clippy::all + clippy::pedantic, carefully `#[allow]` noise
└── Embedded / unsafe-heavy → above + selected `clippy::restriction` lints (e.g. `clippy::unwrap_used`)
```

NEVER enable `clippy::restriction` as a group; always pick individual lints. Restriction lints contradict each other by design.

### "Where do I configure lints?"

```
Configure-once across crate?
├── Cargo.toml [lints.clippy] (since 1.74)  → preferred
├── Cargo.toml [lints.rust]                 → rustc warnings
└── crate-root attribute `#![warn(clippy::pedantic)]` → also valid, but less discoverable

Per-line allow?
└── `#[allow(clippy::lint_name)]` directly above the item.
```

Source : https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section.

### "Which rustc optimization level?"

```
Profile?
├── dev (default for `cargo build`)              → opt-level = 0, full debug info
├── release (default for `cargo build --release`)→ opt-level = 3, line-tables-only
├── Size-constrained binary                       → opt-level = "z" (smallest) or "s" (small)
└── Production with reasonable size + speed       → opt-level = 3, LTO = "thin"
```

Profile defaults : https://doc.rust-lang.org/cargo/reference/profiles.html.

## Patterns

### Pattern : pin a toolchain in CI and locally

ALWAYS commit `rust-toolchain.toml` so `rustup` resolves the same toolchain in CI and on developer machines.

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.85.0"
components = ["rustfmt", "clippy", "rust-analyzer"]
profile = "minimal"
```

When CI runs `cargo check`, `rustup` first installs `1.85.0` with the listed components, then dispatches to it. NEVER rely on the host's default toolchain in CI.

### Pattern : cross-compile to a new target

```bash
rustup target add aarch64-apple-darwin
cargo build --release --target aarch64-apple-darwin
```

For non-trivial targets (Windows-from-Linux, bare-metal), a target-specific linker plus `.cargo/config.toml` is required. See `[[rust-impl-cargo-project]]` for cross-compile workflows.

### Pattern : project-level lint configuration

In `Cargo.toml` (Rust 1.74+) :

```toml
[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "deny"
```

The `priority` field lets group-level lints (`pedantic`) be overridden by specific lints. Source : https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section.

### Pattern : rustfmt configuration

```toml
# rustfmt.toml
edition = "2024"
max_width = 100
use_field_init_shorthand = true
imports_granularity = "Crate"  # nightly-only option as of 1.85
```

ALWAYS set `edition` in `rustfmt.toml` explicitly. Stable rustfmt ignores unstable options without error unless invoked with `+nightly`. Source : https://rust-lang.github.io/rustfmt/.

### Pattern : MSRV declaration

Declare the Minimum Supported Rust Version in `Cargo.toml` :

```toml
[package]
name = "my-crate"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
```

`cargo build` refuses to compile with a rustc older than `rust-version`. Since Rust 1.84, the MSRV-aware resolver (opt-in via `resolver.incompatible-rust-versions = "fallback"` in `.cargo/config.toml`) selects dep versions compatible with your MSRV. Source : https://blog.rust-lang.org/2025/01/09/Rust-1.84.0/.

### Pattern : enforce clippy in CI

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

`-D warnings` promotes every warning (including warn-by-default clippy categories) to a hard error. ALWAYS run this in CI for libraries.

## Reference Links

- `references/methods.md` : Full surface of `rustup`, `cargo`, `rustc`, clippy lint categories, rustfmt options.
- `references/examples.md` : End-to-end snippets for install, channel switch, cross-compile, lint config, MSRV setup, rust-toolchain.toml.
- `references/anti-patterns.md` : Common toolchain mistakes with WHY-this-fails explanations.

## Cross-References

- `[[rust-core-language-versions]]` : Channels-as-concept, editions, MSRV-as-policy, version matrix.
- `[[rust-syntax-edition-2024]]` : Language-level edition 2024 changes consumed via the toolchain.
- `[[rust-impl-cargo-project]]` : Cargo.toml manifest details, workspaces, features, `[lints]` table, profiles.
- `[[rust-impl-testing]]` : Test runners, `cargo test`, `cargo bench`.

## Source Verification

All claims verified against :

- https://rust-lang.github.io/rustup/concepts/toolchains.html (rustup model)
- https://rust-lang.github.io/rustup/concepts/components.html (components)
- https://rust-lang.github.io/rustup/overrides.html (rust-toolchain.toml)
- https://doc.rust-lang.org/cargo/commands/index.html (cargo subcommands)
- https://doc.rust-lang.org/cargo/reference/profiles.html (profiles + opt-level)
- https://doc.rust-lang.org/cargo/reference/manifest.html (`[lints]`, `rust-version`)
- https://rust-lang.github.io/rust-clippy/master/ (lint categories)
- https://rust-lang.github.io/rustfmt/ (rustfmt options)
- https://blog.rust-lang.org/2025/01/09/Rust-1.84.0/ (MSRV-aware resolver)
- https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/ (Edition 2024 stable, cargo new defaults)

Last verified : 2026-05-19.
