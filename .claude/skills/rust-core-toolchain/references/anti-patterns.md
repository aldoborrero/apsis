# Anti-Patterns : Rust Toolchain

Common mistakes when working with the Rust toolchain. Each anti-pattern is paired with WHY it fails and the correct alternative.

## 1. Using nightly for production without justification

**Anti-pattern** :

```toml
# rust-toolchain.toml
[toolchain]
channel = "nightly"
```

```bash
cargo build --release
# Ship the resulting binary to prod.
```

**Why this fails** :

- Nightly receives unstable feature changes daily; tomorrow's nightly may break today's build.
- The stable Rust promise (no breakage in stable APIs) does NOT apply to nightly.
- A nightly-only feature can be removed or redesigned at any time, leaving your code unbuildable.
- Many crates explicitly refuse to build on nightly via `#[cfg(not(stable))]` style checks for soundness reasons.

**Correct alternative** : Use stable. If a specific unstable feature is required (e.g. a procedural macro that needs `#![feature(proc_macro_diagnostic)]`), pin to a specific nightly date AND document the reason :

```toml
# rust-toolchain.toml
[toolchain]
# Pinned for cargo-fuzz which requires `-Z sanitizer=address` (2026-Q1 status).
# Re-evaluate when address-sanitizer stabilizes.
channel = "nightly-2025-05-15"
components = ["rust-src", "llvm-tools"]
```

NEVER use unpinned `nightly` for code you ship.

## 2. Missing `rust-toolchain.toml`, causing CI to diverge from dev

**Anti-pattern** :

The developer has `rustc 1.87.0` locally. CI uses `actions/setup-rust` with no version, defaulting to whatever `stable` was at the time the cache last ran (could be 1.82, 1.85, 1.87, anything). Builds pass locally but fail randomly in CI when a clippy lint changes behavior between versions.

**Why this fails** :

- Rust ships every 6 weeks. Clippy lints, type inference, MSRV checks, and even codegen change between versions.
- Without a project-pinned toolchain, "works on my machine" is a guarantee that decays over time.
- CI cache misses leak the latest stable into a build that was tested against an older one.

**Correct alternative** : Commit `rust-toolchain.toml` AND configure CI to respect it :

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.85.0"
components = ["rustfmt", "clippy"]
```

```yaml
# CI step that respects rust-toolchain.toml
- uses: dtolnay/rust-toolchain@stable
```

`dtolnay/rust-toolchain` (and most actions) prefer `rust-toolchain.toml` when present. Now local dev and CI build with bit-identical toolchains.

## 3. Ignoring or silencing `clippy::correctness`

**Anti-pattern** :

```rust
#![allow(clippy::correctness)]  // make clippy quiet

fn parse_id(s: &str) -> u32 {
    s.parse().unwrap()
}
```

Or via Cargo.toml :

```toml
[lints.clippy]
correctness = "allow"
```

**Why this fails** :

- The `correctness` category is `deny`-by-default specifically because every lint in it flags code that is objectively wrong : eq-on-NaN, double-parens, swap-with-temporary, comparisons that always return the same value, etc.
- Silencing the category at crate level masks real bugs the moment one slips in.
- A `correctness` warning is a bug report, not a style preference. Fixing the underlying code is ALWAYS the right action.

**Correct alternative** : NEVER silence `correctness` at crate level. If a specific correctness lint produces a false positive on a single line, allow it locally with a comment :

```rust
// `clippy::float_cmp` here is intentional: we compare against an exact constant.
#[allow(clippy::float_cmp)]
fn is_pi(x: f64) -> bool {
    x == std::f64::consts::PI
}
```

And open an issue against clippy if it really is a false positive : https://github.com/rust-lang/rust-clippy/issues.

## 4. Using `cargo fmt` to fix semantic issues

**Anti-pattern** :

```bash
cargo test
# 13 failures
cargo fmt
cargo test
# Still 13 failures, but pretty.
```

The developer assumes `cargo fmt` will "clean up" code and incidentally fix compile errors, or expects `cargo fmt` to apply clippy suggestions.

**Why this fails** :

- `rustfmt` ONLY rearranges whitespace and shape : indentation, line wrapping, comma placement, brace style. It NEVER changes the meaning of code, NEVER renames things, NEVER inserts or removes statements.
- Tools have orthogonal responsibilities : `cargo fmt` = layout, `cargo clippy --fix` = semantic suggestions, `cargo fix` = compiler-suggested fixes (edition migration, deprecation), `cargo check` / `cargo build` = compile errors.
- Conflating them produces magical-thinking workflows where developers run "all the tools" without understanding which one does what.

**Correct alternative** : Use the right tool for each job :

```bash
cargo fmt                            # whitespace only
cargo fix --edition                  # migrate to next edition
cargo clippy --fix                   # apply clippy suggestions
cargo check                          # find compile errors fast
cargo test                           # find semantic bugs
```

ALWAYS read the diff that `cargo fix` / `cargo clippy --fix` produces before committing it. Auto-fix is a starting point, not a final answer.

## 5. Pinning to `nightly-YYYY-MM-DD` without explaining why

**Anti-pattern** :

```toml
# rust-toolchain.toml
[toolchain]
channel = "nightly-2024-03-15"
```

No comment, no reference, no documentation. Two years later, nobody knows why this is pinned, whether it can be updated, or what feature depended on this exact nightly.

**Why this fails** :

- Pinned nightlies have ALL the unstable-feature-removal risk of `nightly`, but compounded by the fact that nobody remembers what the pin was for.
- Updating the pin to a fresher nightly becomes risky, because you don't know what features were assumed.
- The pin can outlive the reason : the unstable feature might have been stabilized in a later release, meaning you could go back to stable, but no one realizes.

**Correct alternative** : ALWAYS comment a nightly pin with the reason, the feature being relied on, and a re-evaluation trigger :

```toml
# rust-toolchain.toml
#
# Pinned: nightly-2025-05-15
# Reason: cargo-fuzz requires `-Z sanitizer=address` for AddressSanitizer.
# Tracking: https://github.com/rust-lang/rust/issues/39699
# Re-evaluate: when AddressSanitizer stabilizes OR every 6 months, whichever first.
[toolchain]
channel = "nightly-2025-05-15"
components = ["rust-src", "llvm-tools"]
```

Better still, move the nightly requirement out of the main toolchain : run `cargo +nightly fuzz` from CI for fuzzing, and keep the project's primary toolchain on stable.

## 6. Mixing global `rustup default` with project requirements

**Anti-pattern** :

```bash
# Developer: "I work mostly on nightly, so..."
rustup default nightly
# Now all projects on this machine use nightly unless they override.
```

Then opens a stable-only project, runs `cargo build`, hits unstable-feature warnings, and starts adding `#[cfg(...)]` workarounds.

**Why this fails** :

- Global default leaks into every project that doesn't pin its own toolchain.
- Bugs that only manifest on nightly will go undetected on this developer's machine.
- Other contributors will see the project working differently for them.

**Correct alternative** : Keep `rustup default stable`. Use per-project pinning :

```bash
rustup default stable                              # global default = stable
cd ~/projects/needs-nightly
rustup override set nightly-2025-05-15            # this project only
# OR commit a rust-toolchain.toml in the project root.
```

`rustup override` lives in a state file under `~/.rustup/settings.toml` and applies only when CWD is the override-set directory. `rust-toolchain.toml` is committed and applies to everyone.

## 7. Enabling `clippy::restriction` as a group

**Anti-pattern** :

```toml
[lints.clippy]
restriction = "warn"
```

**Why this fails** :

- `clippy::restriction` is a collection of opt-in coding rules that contradict each other by design : `clippy::unwrap_used` forbids `.unwrap()`, but `clippy::missing_errors_doc` requires documenting errors; `clippy::implicit_return` forbids `return`, but `clippy::needless_return` forbids implicit return.
- The clippy book explicitly states : "the restriction group is NOT meant to be enabled wholesale".
- You will get thousands of warnings, most of which contradict each other, and the noise will drown out real signals.

**Correct alternative** : Pick individual restriction lints relevant to your project :

```toml
[lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
# Individually opt-in restriction lints:
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
indexing_slicing = "warn"
```

Document why each restriction lint is enabled (e.g. "no panics in this lib, it runs in kernel-space").

## 8. Running `cargo build --release` in dev loop

**Anti-pattern** :

```bash
# 100 times per day:
cargo build --release
# Wait 60 seconds per change for incremental release build.
```

**Why this fails** :

- The `release` profile sets `opt-level = 3` and disables incremental compilation by default. Codegen is the slowest pipeline stage.
- For dev iteration (type-check + run), the `dev` profile finishes 5-10x faster.
- For pure type-checking, `cargo check` skips codegen entirely and is the fastest mode.

**Correct alternative** : Match the command to the goal.

```bash
cargo check                          # fastest: syntax + types, no codegen
cargo build                          # next: dev profile, full codegen, debug info
cargo build --release                # slowest: release profile, used for ship + perf
```

The dev-loop ladder : `cargo check` -> `cargo test` -> `cargo build --release` (only when measuring perf or shipping).
