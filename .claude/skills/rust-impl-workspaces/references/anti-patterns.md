# rust-impl-workspaces: anti-patterns

Twelve documented anti-patterns. Each one : symptom -> WHY it fails -> FIX. Sourced from the Cargo book ([workspaces reference](https://doc.rust-lang.org/cargo/reference/workspaces.html), [resolver reference](https://doc.rust-lang.org/cargo/reference/resolver.html)) and real-world drift between standalone Cargo projects.

---

## 1. Duplicated dependency versions across members

Symptom : `crates/a/Cargo.toml` has `serde = "1.0.190"`, `crates/b/Cargo.toml` has `serde = "1"`, `crates/c/Cargo.toml` has `serde = "=1.0.182"`. A `cargo update -p serde` upgrades some members and not others.

WHY it fails : every version spec is independent. The lockfile picks one resolved version (because all version reqs are compatible) but the **caret semantics** differ. The pinned `=1.0.182` blocks the workspace from receiving security fixes for 1.0.183+. Worse, when someone bumps `crates/a` to `2.0` but forgets `crates/b`, the two members compile against different major versions of `serde` and types do not interoperate.

FIX :

```toml
# Root Cargo.toml
[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
```

```toml
# Every member Cargo.toml
[dependencies]
serde = { workspace = true }
```

---

## 2. Forgotten `resolver` in virtual workspace

Symptom : a virtual workspace works, but `cargo build` enables features for dev-dependencies in unexpected places. Switching `tokio = { version = "1", features = ["full"] }` to a narrower feature set still pulls in the kitchen sink.

WHY it fails : with no `[package]` section at the root, Cargo cannot infer the resolver from `package.edition` and falls back to resolver `"1"`. Resolver 1 unifies features across **all** targets (build / dev / target-specific), so a dev-dep `features = ["X"]` activates `X` in the normal build too.

FIX :

```toml
# Root Cargo.toml
[workspace]
resolver = "3"          # explicit, edition-2024-aware
members = ["crates/*"]
```

ALWAYS write `resolver` in every virtual workspace. Pick `"3"` on Rust 1.84+, `"2"` for older toolchains.

---

## 3. `resolver` on a member manifest

Symptom : developer adds `resolver = "2"` to `crates/legacy/Cargo.toml` hoping to opt that one crate out of resolver 3 quirks. Behaviour does not change.

WHY it fails : the resolver is a workspace-global property. Cargo reads it **only** from the root manifest. Member-level `resolver` keys are ignored (and produce a warning in recent Cargo).

FIX : you cannot have two resolvers in one workspace. If a single member truly needs a different resolver, it must be split into its own workspace (typically by removing it from `members` and giving it its own `Cargo.toml` with a separate `target/`).

---

## 4. Mixing `[workspace]` and `[package]` at root for a peer-multi-crate project

Symptom : the root `Cargo.toml` has both `[package] name = "monorepo"` and `[workspace] members = [...]`, but no actual code lives at the root - `src/lib.rs` is just `pub fn dummy() {}`.

WHY it fails : the root crate compiles, publishes accidentally, and shows up in `cargo tree` confusingly. Documentation tools generate a useless doc page for the empty root. Some CI scripts assume the root manifest IS the main artifact.

FIX : convert to a virtual workspace. Delete root `[package]` and `src/`. Move any genuine code into `crates/<name>/`.

```toml
# Root Cargo.toml (after)
[workspace]
resolver = "3"
members  = ["crates/*"]
```

---

## 5. Unexplained `exclude`

Symptom : `exclude = ["crates/legacy"]` in the root manifest with no comment. Six months later a new contributor removes the line "to clean up", CI breaks because `legacy` no longer compiles.

WHY it fails : without a recorded reason, the next reader cannot judge whether the exclude is still needed. They infer (wrongly) that it is dead code.

FIX :

```toml
[workspace]
members = ["crates/*"]
exclude = [
    "crates/legacy",    # API frozen, kept for ABI reference. Do not re-include without owner sign-off.
    "crates/scratch",   # local experiments, not part of build/CI.
]
```

ALWAYS comment every `exclude` entry. ALWAYS comment every `default-members` entry whose absence from the default set is non-obvious.

---

## 6. Per-member dev-dependency drift

Symptom : `crates/a` uses `proptest = "1.4"`, `crates/b` uses `proptest = "1.2"`, `crates/c` uses `quickcheck` instead. Test patterns diverge; refactoring tests across crates is hard.

WHY it fails : dev-dependencies are dependencies. They drift the same way prod deps drift. The fix is identical, but developers often forget that `[workspace.dependencies]` covers dev-deps too.

FIX :

```toml
# Root
[workspace.dependencies]
proptest = "1"
tempfile = "3"
insta    = "1"

# Member
[dev-dependencies]
proptest = { workspace = true }
tempfile = { workspace = true }
insta    = { workspace = true }
```

---

## 7. Missing `[workspace.package].edition` -> edition mismatch

Symptom : the workspace was started in 2023 with edition 2021. Two new crates added in 2025 have `edition = "2024"` because `cargo new` defaults to it. Within the same workspace, half the crates use 2024 features (`unsafe extern`, RPIT lifetime capture) and half do not.

WHY it fails : nothing forces consistency. Each member's `edition` is its own. `cargo build` succeeds in both editions, so the discrepancy is invisible until a code review notices `extern fn` instead of `unsafe extern fn` in one member.

FIX : set `edition` once in `[workspace.package]` and inherit everywhere.

```toml
# Root
[workspace.package]
edition = "2024"
rust-version = "1.85"

# Every member
[package]
edition.workspace      = true
rust-version.workspace = true
```

When migrating an older workspace, run `cargo fix --edition --workspace` first.

---

## 8. `[profile.release]` in a member manifest

Symptom : `crates/server/Cargo.toml` has its own `[profile.release] lto = "fat"`, but binary size of the workspace-built server is unchanged.

WHY it fails : profile sections are recognised **only** in the root manifest. Cargo emits a warning (recent versions) and ignores the member-level entry. The release profile applied is the root's, which defaults to `lto = false`.

FIX :

```toml
# Root Cargo.toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = "symbols"
```

The profile applies to **all** members compiled with `--release`. If a single member needs a different profile, use a `[profile.release-server] inherits = "release"` custom profile in the root and invoke `cargo build -p server --profile release-server`.

---

## 9. `optional = true` inside `[workspace.dependencies]`

Symptom : `serde = { version = "1", optional = true }` under `[workspace.dependencies]`. Cargo errors at workspace load.

WHY it fails : the Cargo manifest reference explicitly forbids `optional` in the workspace-level dep spec. Optionality is a per-member decision (one member wants `serde` as a feature flag, another wants it always-on).

FIX :

```toml
# Root
[workspace.dependencies]
serde = { version = "1", features = ["derive"] }

# Member that wants serde optional
[dependencies]
serde = { workspace = true, optional = true }

[features]
default = []
json    = ["dep:serde"]
```

---

## 10. Enumerating members instead of globbing

Symptom : `members = ["crates/a", "crates/b", "crates/c", "crates/d", ...]` with 18 entries. Every `cargo new --lib crates/new` requires a manifest edit; PR reviewers must remember to ask "did you add it to members?".

WHY it fails : pure churn. The pattern provides no value once the count exceeds three or four, and forgetting it produces a member that "exists in the file system but is not part of the workspace".

FIX :

```toml
[workspace]
members = ["crates/*"]
```

If a subset of `crates/*` truly should not be members, list them in `exclude` (with a comment, per anti-pattern #5).

---

## 11. Holding a different lockfile per crate

Symptom : the repo has `crates/a/Cargo.lock`, `crates/b/Cargo.lock`, and `crates/c/Cargo.lock`. Build times triple because each crate has its own `target/`. Versions drift even with `[workspace.dependencies]` because the crates are not actually a workspace yet.

WHY it fails : without a root `[workspace]` declaration, three sibling `Cargo.toml` files are three independent packages. Each runs its own resolver, produces its own `Cargo.lock`, builds in its own `target/`. No deduplication.

FIX : add a root `Cargo.toml` with `[workspace] members = ["crates/*"] resolver = "3"`. Delete every member's `Cargo.lock`. Delete every member's `target/`. Run `cargo build --workspace` once; a single root `Cargo.lock` and `target/` appear. Commit only the root lockfile.

ALWAYS git-ignore `target/` (at the root only; child `target/` should not exist after the workspace is in place).

---

## 12. `[lints]` selective override misunderstanding

Symptom : a member writes:

```toml
[lints]
workspace = true

[lints.clippy]
unwrap_used = "allow"      # tries to override one lint while inheriting the rest
```

The override silently does nothing.

WHY it fails : `[lints] workspace = true` inherits the **entire** lint configuration. You cannot inherit-then-override individual lints. Cargo currently treats the table as atomic.

FIX : a member that needs a different lint set drops the `workspace = true` line and writes a full standalone `[lints]` table.

```toml
# Member (legitimate exception, fully self-defined lints)
[lints.rust]
unsafe_code = "forbid"
unused_imports = "warn"

[lints.clippy]
unwrap_used = "allow"      # this member calls unwrap() in test helpers
needless_pass_by_value = "warn"
```

ALWAYS leave a comment explaining why a member opts out of the shared lints. If many members override the same thing, that thing belongs in `[workspace.lints]` instead.
