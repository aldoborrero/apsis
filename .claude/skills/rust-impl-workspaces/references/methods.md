# rust-impl-workspaces: methods and keys reference

Complete reference for every key Cargo recognises under `[workspace]`, `[workspace.package]`, `[workspace.dependencies]`, `[workspace.lints]`, and the inheritance syntax in member manifests. All entries verified against [Cargo book: Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html) and [Cargo book: Manifest format](https://doc.rust-lang.org/cargo/reference/manifest.html).

---

## `[workspace]` section

Lives in the **root** `Cargo.toml` only. A manifest containing `[workspace]` is the workspace root; all paths in `members` are interpreted relative to it.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `resolver` | string `"1"`, `"2"`, `"3"` | inferred from `package.edition` of root, else `"1"` | Selects the feature resolver. In a virtual workspace there is no `package.edition` so this **must be set explicitly** for any resolver other than `"1"`. |
| `members` | array of path strings, globs allowed | `[]` | Crates to include in the workspace. Glob patterns `*` and `?` are expanded. Any `path = "..."` dependency from a member is also implicitly a member. |
| `default-members` | array of path strings | `members` | Crates operated on by `cargo build`, `cargo test`, etc. when no `-p` / `--workspace` flag is given. Must be a subset of `members`. |
| `exclude` | array of path strings | `[]` | Paths to remove from the member set (typically used to skip directories captured by a `members` glob). |
| `package` | inline table -> see `[workspace.package]` | none | Shared package metadata inheritable by members. |
| `dependencies` | inline table -> see `[workspace.dependencies]` | none | Shared dependency specifications inheritable by members. |
| `lints` | inline table -> see `[workspace.lints]` | none | Shared lint configuration inheritable by members. Requires Rust 1.74+. |
| `metadata` | arbitrary TOML table | none | Free-form metadata for external tools. Cargo ignores its contents. |

### MSRV notes

- `[workspace.dependencies]` and `[workspace.package]` inheritance : Rust **1.64**.
- `[workspace.lints]` : Rust **1.74**.
- Resolver `"2"` : Rust **1.51**.
- Resolver `"3"` (MSRV-aware) : Rust **1.84**.
- Edition 2024 defaults to resolver `"3"` : Rust **1.85**.

---

## `[workspace.package]` keys (inheritable metadata)

Every key listed here can be set in `[workspace.package]` at the root and inherited by a member via `field.workspace = true`. Source: Cargo manifest reference, "The workspace.package table".

| Key | Type | Notes |
|-----|------|-------|
| `version` | string (semver) | Members may also override locally; inheritance is opt-in per key. |
| `edition` | `"2015"` / `"2018"` / `"2021"` / `"2024"` | Set once for the whole workspace. |
| `rust-version` | string (semver-style, e.g. `"1.85"`) | Combined with resolver `"3"` enables MSRV-aware resolution. |
| `authors` | array of strings | |
| `description` | string | |
| `documentation` | URL string | |
| `homepage` | URL string | |
| `repository` | URL string | |
| `license` | SPDX expression string | |
| `license-file` | path string | Mutually exclusive with `license`. |
| `readme` | path string or `false` | |
| `keywords` | array of strings (max 5) | crates.io constraint. |
| `categories` | array of strings (max 5) | crates.io constraint. |
| `include` | array of glob strings | Files to include when publishing. |
| `exclude` | array of glob strings | Files to exclude when publishing. Distinct from `[workspace].exclude`. |
| `publish` | bool or array of registry names | Prevents accidental publish. |

ALWAYS inherit `edition` and `rust-version` from `[workspace.package]`. They are the two values most prone to drift, and the compiler does not flag a mismatch.

---

## `[workspace.dependencies]` syntax

Each entry has the same shape as a normal `[dependencies]` entry, with **one restriction** and several inheritance hooks.

### Allowed shapes

```toml
[workspace.dependencies]
# bare version string
serde = "1"

# inline table with version and features
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

# git dependency
my-lib = { git = "https://example.com/repo.git", tag = "v0.3.0" }

# path dependency
my-core = { path = "../my-core" }

# registry dependency
my-pkg = { version = "2", registry = "my-registry" }
```

### Not allowed

```toml
[workspace.dependencies]
serde = { version = "1", optional = true }   # ERROR: optional not allowed here
```

`optional = true` MUST be set on the **member-level** entry, not the workspace-level entry.

### Member-side inheritance

```toml
# member Cargo.toml
[dependencies]
# Plain inheritance
serde = { workspace = true }

# Shorthand using dotted key
anyhow.workspace = true

# Inheritance with member-level feature additions
tokio = { workspace = true, features = ["fs", "process"] }

# Inheritance + opt-in
serde = { workspace = true, optional = true }

# Inheritance + disable default features
regex = { workspace = true, default-features = false }
```

ALLOWED to add at the member level: `features`, `optional`, `default-features`. NOT allowed to override at the member level: `version`, `git`, `path`, `registry`, `branch`, `tag`, `rev`.

Same syntax applies under `[build-dependencies]` and `[dev-dependencies]`.

---

## `[workspace.lints]` syntax

Lint groups are namespaced by tool. Cargo accepts any tool name; `rust`, `clippy`, and `rustdoc` are the built-ins.

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
unused_imports = "deny"
missing_docs = { level = "warn", priority = -1 }

[workspace.lints.clippy]
unwrap_used = "warn"
needless_pass_by_value = "deny"
pedantic = { level = "warn", priority = -1 }
```

Level values: `"forbid"`, `"deny"`, `"warn"`, `"allow"`. `priority` defaults to `0`; lower numbers take effect first so a group can be set then individual lints overridden.

Member-side:

```toml
[lints]
workspace = true
```

The entire workspace lint table is inherited. A member that wants different lints MUST drop the `workspace = true` line and write its own `[lints]` table; selective override is not supported.

---

## Resolver behaviour

| Resolver | Edition default | MSRV | Behaviour |
|----------|-----------------|------|-----------|
| `"1"` | 2015 / 2018 | always | Legacy feature unification (all targets unify together). |
| `"2"` | 2021 | Rust 1.51 | Target/build-dep/dev-dep dependency unification scopes split. |
| `"3"` | **2024** | Rust 1.84 | Resolver `"2"` + `resolver.incompatible-rust-versions` defaults to `"fallback"`. |

The `resolver` key is recognised on:

- The `[workspace]` table of a virtual workspace.
- The `[package]` table of a root-package workspace (or single-crate manifest).

It is **ignored** on a member manifest of a workspace; the root resolver always wins.

`fallback` semantics (resolver `"3"`): when a dependency has multiple compatible versions and only some satisfy the declared `rust-version` of the package, Cargo prefers the highest version compatible with that MSRV.

---

## Root-only manifest keys

These keys are recognised **only** in the root manifest. Entries in member manifests are ignored (with a warning in recent Cargo).

- `[profile.<name>]` (`dev`, `release`, `test`, `bench`, custom)
- `[patch.<source>]`
- `[replace]`
- `[workspace.metadata.*]`

ALWAYS move a `[profile.release] lto = "fat"` etc. block to the root manifest. A member-level profile override is silently dropped.

---

## Workspace shape decision matrix

| Project type | Recommended layout |
|--------------|--------------------|
| Single crate, one binary | No workspace. Just `cargo new`. |
| Single crate, library + integration tests | No workspace. |
| Library + thin binary in same repo | Root-package workspace (root = library, `members = ["cli"]`). |
| Library + multiple unrelated binaries | Virtual workspace. |
| Microservices monorepo | Virtual workspace with `members = ["services/*"]`. |
| Plugin host + N plugins | Virtual workspace, plugins under `plugins/*`, host under `host/`. |
| Workspace + vendored fork | Virtual workspace, fork in `vendor/<name>` listed in `exclude`. |

---

## CLI flags relevant to workspaces

| Flag | Effect |
|------|--------|
| `--workspace` (alias `--all`) | Apply the command to every member, ignoring `default-members`. |
| `-p <name>` / `--package <name>` | Apply to a single member by package name. May be repeated. |
| `--exclude <name>` | Combined with `--workspace`, skip a member. |
| `--manifest-path <path>` | Operate on a non-cwd manifest; useful for `path/to/workspace/Cargo.toml`. |

For features:

| Flag | Effect |
|------|--------|
| `--features <a,b>` | Member-context: enable features of the selected package. |
| `--all-features` | Enable every feature of the selected packages. |
| `--no-default-features` | Disable the `default` feature on selected packages. |

Workspace-wide feature selection works with `-p`-style targeting; a single `--features X` applies to **all** selected packages.
