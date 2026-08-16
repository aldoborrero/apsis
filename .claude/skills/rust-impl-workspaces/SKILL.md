---
name: rust-impl-workspaces
description: >
  Use when the user organises multiple crates into a Cargo workspace, sets up `[workspace.dependencies]` inheritance, manages member crates, chooses resolver 2 vs 3, or hits dependency drift across workspace members.
  Prevents dependency drift, repeated edition / rust-version specs across members, target-directory clutter, and accidental member-exclude.
  Covers: virtual workspace (top-level Cargo.toml with `[workspace]` only), workspace inheritance (`workspace = true`, `[workspace.dependencies]`, `[workspace.package]`, `[workspace.lints]`), member discovery (`members = []`, `default-members`, `exclude`), shared target directory, resolver versions 2 and 3 (when each matters), member-specific dependencies dev vs prod.
  Keywords: workspace, "Cargo workspace", "[workspace]", "virtual manifest", "workspace.dependencies", "workspace.package", "workspace.lints", "workspace = true", inheritance, "default-members", "exclude member", "shared target", "resolver = 2", "resolver = 3", "MSRV resolver", "multi-crate", monorepo, "monorepo Rust", "workspace member".
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# rust-impl-workspaces

A **Cargo workspace** is one or more packages (the members) that share a single `Cargo.lock`, a single `target/` directory, and a single root manifest. For any project larger than one crate (binary + reusable library, a CLI plus its core, a monorepo of microservices, a plugin host plus plugins), workspaces are the **only** correct organisational unit. Without them you get duplicate `target/` directories, drifting dependency versions, duplicated metadata, and lockfile divergence across crates.

Cross-references: [[rust-impl-cargo-project]] (single-crate `Cargo.toml` basics, profiles, features), [[rust-core-language-versions]] (resolver 3 ships with Rust 1.84; edition 2024 defaults to it), [[rust-core-toolchain]] (`rust-toolchain.toml` and rustup channels for the whole workspace).

---

## When to use this skill

- The user has, or is about to have, **more than one crate** in the same repository.
- A binary depends on a library that lives in the same repo.
- The user asks "how do I share `serde = "1"` across all my crates without copy-pasting the version".
- The user wants to set `edition = "2024"` once and not in every member.
- The user is choosing between `resolver = "2"` and `resolver = "3"`.
- The user hits "two different versions of `serde` in one build" (lockfile drift between standalone Cargo projects in the same repo, fixed by promoting them to a workspace).
- The user sees `target/` appearing in three different crate directories.

---

## Quick reference

| Concept | Where it lives | One-line rule |
|---------|----------------|---------------|
| `[workspace]` | root `Cargo.toml` | Declares this manifest as the workspace root. |
| Root-package workspace | root has both `[workspace]` and `[package]` | Top-level crate is itself a member. |
| Virtual workspace | root has `[workspace]` and **no** `[package]` | Preferred for multi-crate repos. |
| `members` | `[workspace]` | List of member paths, supports glob `crates/*`. |
| `default-members` | `[workspace]` | Subset operated on when no `-p` flag given. |
| `exclude` | `[workspace]` | Paths inside `members` globs to skip. |
| `resolver` | `[workspace]` (virtual) or `[package]` (root-package) | Feature-resolution + MSRV-aware behaviour. |
| `[workspace.package]` | root `Cargo.toml` | Shared package metadata (edition, license, version, etc.). |
| `[workspace.dependencies]` | root `Cargo.toml` | Single source of truth for dependency versions. |
| `[workspace.lints]` | root `Cargo.toml` | Shared lint configuration. |
| `field.workspace = true` | member `Cargo.toml` | Inherit `field` from `[workspace.package]`. |
| `dep = { workspace = true }` | member `Cargo.toml` | Inherit `dep` from `[workspace.dependencies]`. |
| `[lints] workspace = true` | member `Cargo.toml` | Inherit lint table. |
| Cargo.lock | root only | One lockfile for the entire workspace. |
| `target/` | root only | One build output directory. |
| `[patch]`, `[replace]`, `[profile.*]` | root only | Member-level entries are ignored. |

---

## The virtual-workspace template (the default you want)

```toml
# Cargo.toml (workspace root)
[workspace]
resolver = "3"
members  = ["crates/*"]
exclude  = ["crates/scratch"]
default-members = ["crates/app"]

[workspace.package]
edition      = "2024"
rust-version = "1.85"
license      = "MIT OR Apache-2.0"
authors      = ["OpenAEC Foundation"]
repository   = "https://github.com/example/repo"

[workspace.dependencies]
serde      = { version = "1", features = ["derive"] }
tokio      = { version = "1", features = ["rt-multi-thread", "macros"] }
anyhow     = "1"
thiserror  = "2"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
unwrap_used = "warn"
```

ALWAYS prefer a **virtual workspace** for any repo with more than one crate. The root `Cargo.toml` has no `[package]` section, so no code lives at the root; every crate is a member under `crates/`.

NEVER mix `[workspace]` and `[package]` at the root unless the project genuinely is "one main crate plus a few helper crates". The virtual layout is easier to reason about, easier to add members to, and never confuses tooling about which crate the root is.

---

## Member manifests: inherit everything you can

```toml
# crates/app/Cargo.toml
[package]
name         = "app"
version      = "0.1.0"
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
authors.workspace      = true
repository.workspace   = true

[dependencies]
serde     = { workspace = true }
tokio     = { workspace = true, features = ["fs"] }
anyhow    = { workspace = true }
my-core   = { path = "../my-core" }

[dev-dependencies]
proptest = "1"

[lints]
workspace = true
```

ALWAYS write `field.workspace = true` for **every** inheritable key (`edition`, `rust-version`, `license`, `authors`, `repository`, `homepage`, `documentation`, `keywords`, `categories`, `description`, `publish`, `readme`, `include`, `exclude`, `version`). The compiler does not warn on a missing inheritance, so drift creeps in silently.

ALWAYS reference shared deps as `dep = { workspace = true }`. You may **add** features at the call site (`features = ["fs"]`) but you cannot override the version: that is the point.

NEVER pin a different version in a member than the one declared in `[workspace.dependencies]`. Cargo errors on the conflict, but the more common mistake is to forget the `workspace = true` and silently get the wrong version because the dependency was declared from scratch.

NEVER set `optional = true` inside `[workspace.dependencies]`. The Cargo reference disallows this. Mark the member-level entry optional instead:

```toml
# member
[dependencies]
serde = { workspace = true, optional = true }
```

---

## `members`, `default-members`, `exclude`

```toml
[workspace]
members         = ["crates/*", "tools/codegen"]
default-members = ["crates/app", "crates/server"]
exclude         = ["crates/scratch", "crates/legacy"]
```

- `members` accepts plain paths and glob patterns (`*`, `?`). Any `path = "../foo"` dependency from a member also auto-becomes a member.
- `default-members` is the **subset** that `cargo build`, `cargo test`, `cargo run` operate on when invoked from the workspace root **without** `-p` or `--workspace`. Useful when the repo has a release-critical core (build by default) and benches / tools (only on demand).
- `exclude` removes a path from `members` even when a glob would have included it. Use it for scratch directories, vendored copies, or examples that should not block CI.

ALWAYS prefer a glob (`crates/*`) over an enumerated list once the count exceeds three. The list is otherwise a churn magnet.

ALWAYS document **why** a path is in `exclude` with a comment in the manifest. An unexplained exclude is the second-most-likely cause of "why doesn't `cargo test --workspace` build my crate".

---

## Resolver: `"2"` or `"3"`

| Resolver | Default for | Stabilised in | What it changes |
|----------|-------------|---------------|-----------------|
| `"1"` | edition 2015/2018 | always | Legacy feature unification. |
| `"2"` | edition 2021 | Rust 1.51 | Feature unification respects target/build/dev dependency boundaries. |
| `"3"` | **edition 2024** | Rust 1.84 | Inherits `"2"` + MSRV-aware resolution (`resolver.incompatible-rust-versions = "fallback"`). |

In edition 2024 (Rust 1.85+) `cargo new` writes `resolver = "3"` for you. The resolver MUST be set on the **root** manifest; member-level `resolver` entries are ignored. In a virtual workspace it lives in `[workspace]`; in a root-package workspace it lives in `[package]`.

ALWAYS write the `resolver` key **explicitly** in virtual workspaces. There is no `package.edition` at the root from which Cargo could infer it, and forgetting it pins you to resolver 1 with confusing feature-unification behaviour.

ALWAYS pick `"3"` for new edition-2024 workspaces. The MSRV-aware behaviour is strictly better when `[workspace.package].rust-version` is set: Cargo selects older compatible dependency versions instead of breaking the build.

NEVER mix resolver versions across members. The root resolver controls the entire workspace; per-member `resolver` keys are silently ignored, which is exactly the wrong failure mode for someone trying to "opt one crate into resolver 2".

---

## Shared `target/` and one `Cargo.lock`

Once the workspace is in place, **every** `cargo` invocation in any subdirectory writes to the root `target/`. This is the headline saving: a clean clone + `cargo build --workspace` reuses every artifact across members. Three previously-separate crates with overlapping dependencies might have produced 3 GB of `target/` directories; one workspace produces 1.2 GB.

There is exactly one `Cargo.lock` at the workspace root. Member crates do not have their own. This guarantees every member sees the **same resolved version** of every dependency. A direct consequence: a member cannot depend on `serde = "1.0"` while another member depends on `serde = "1.1"` and end up with both at build time. They share one resolved `serde` version.

Root-only configuration items (per the Cargo reference):

- `[patch.*]`, `[replace]`
- `[profile.*]` (including custom profiles)
- `[workspace.metadata]`

Entries of these tables **in member manifests are ignored**, with a warning in recent Cargo. ALWAYS move them to the root.

---

## Decision tree: workspace layout

```
Do you have more than one crate in this repo?
|
+-- No: skip this skill. See [[rust-impl-cargo-project]].
|
+-- Yes: is there a single dominant binary or library that *is* the project,
|        and the others are small helpers?
|   |
|   +-- Yes: root-package workspace.
|   |        Root Cargo.toml has both [package] and [workspace].
|   |        Helpers live in subdirs and are listed in members.
|   |
|   +-- No / multiple peers: VIRTUAL workspace (default).
|            Root Cargo.toml has only [workspace] (no [package]).
|            All crates under crates/, members = ["crates/*"].
|
+-- Do you share dependency versions across crates?
|   +-- Yes: declare them in [workspace.dependencies], reference via workspace = true.
|
+-- Do members share metadata (edition, license, rust-version, authors)?
|   +-- Yes: declare in [workspace.package], inherit via field.workspace = true.
|
+-- Do members share lint configuration?
|   +-- Yes: declare in [workspace.lints], inherit via [lints] workspace = true.
|
+-- Pick resolver:
    edition 2024 -> "3".
    edition 2021 only -> "2".
```

---

## Common operations

| Goal | Command |
|------|---------|
| Build every member | `cargo build --workspace` |
| Build only `default-members` | `cargo build` (from workspace root) |
| Build one member | `cargo build -p my-core` |
| Test every member | `cargo test --workspace` |
| Test one member only | `cargo test -p my-core` |
| Exclude a member from a workspace command | `cargo test --workspace --exclude legacy` |
| Update a single dep workspace-wide | `cargo update -p serde` |
| Show the resolved graph | `cargo tree --workspace` |
| Add a member via Cargo | `cargo new --lib crates/new-thing` (then add to `members` if not glob-matched) |

ALWAYS prefer `cargo --workspace` for CI test commands. A bare `cargo test` from the root only covers `default-members`.

---

## Avoid these mistakes

(Full anti-pattern list with WHY and FIX in `references/anti-patterns.md`.)

- Duplicating dependency versions across members instead of `[workspace.dependencies]` (version drift, slow upgrades).
- Forgetting to set `resolver` explicitly in a virtual workspace (silently pins to resolver 1).
- Setting `resolver` on a member manifest (ignored; behaviour does not change).
- Combining `[workspace]` and `[package]` at the root for a multi-crate project that has no obvious dominant crate (use virtual).
- Excluding members with no comment explaining why (next contributor undoes it for the wrong reason).
- Per-member dev-dependency version drift (declare common test deps in `[workspace.dependencies]` too).
- Forgetting `[workspace.package].edition` and getting edition mismatch between members.
- Placing `[profile.release]` in a member manifest (ignored; profile lives only at the root).
- Declaring `optional = true` inside `[workspace.dependencies]` (not allowed by Cargo).
- Listing every member by name instead of using a glob (churns the root manifest on every new crate).

---

## Reference links

[cargo-workspaces]: https://doc.rust-lang.org/cargo/reference/workspaces.html
[cargo-resolver]: https://doc.rust-lang.org/cargo/reference/resolver.html
[cargo-manifest]: https://doc.rust-lang.org/cargo/reference/manifest.html
[cargo-inheriting]: https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#inheriting-a-dependency-from-a-workspace
[cargo-features]: https://doc.rust-lang.org/cargo/reference/features.html
[rust-185]: https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/
[rust-184]: https://blog.rust-lang.org/2025/01/09/Rust-1.84.0/

- [Cargo book: Workspaces][cargo-workspaces]
- [Cargo book: Resolver versions][cargo-resolver]
- [Cargo book: Manifest format][cargo-manifest]
- [Cargo book: Inheriting a dependency from a workspace][cargo-inheriting]
- [Cargo book: Features][cargo-features]
- [Rust 1.85 release (edition 2024 stable)][rust-185]
- [Rust 1.84 release (resolver 3 stable)][rust-184]

For deeper drill-downs see:

- `references/methods.md`: complete table of every `[workspace.*]` key, valid value range, MSRV.
- `references/examples.md`: full three-member workspace, root + member manifests, lint config, profile overrides.
- `references/anti-patterns.md`: 12 documented anti-patterns with WHY and FIX.
