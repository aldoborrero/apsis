---
name: rust-errors-thiserror-anyhow
description: >
  Use when the user chooses an error-handling crate: `thiserror` for structured library errors, `anyhow` for opaque application errors, both together, or migrates between them.
  Prevents using anyhow in a public library API, using thiserror where a one-off opaque error suffices, and losing the error source chain.
  Covers: when to use thiserror (library, structured, callers match on variants) vs anyhow (application, opaque, callers display), thiserror derive (`#[derive(Error)]`, `#[error("...")]`, `#[from]`, `#[source]`, `#[transparent]`), anyhow API (`anyhow::Result`, `Context` trait `.context()` / `.with_context()`, `bail!`, `ensure!`, `anyhow!`), `Error::downcast_ref` for recovering concrete types, combining thiserror at library boundary with anyhow at the app boundary, migration paths.
  Keywords: thiserror, anyhow, "#[derive(Error)]", "#[error(...)]", "#[from]", "#[source]", "#[transparent]", "anyhow::Result", Context, ".context()", ".with_context()", "bail!", "ensure!", "anyhow!", downcast_ref, "library error", "application error", "which error crate", "thiserror vs anyhow", "error crate", "color-eyre", "eyre".
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# rust-errors-thiserror-anyhow

Chooses and applies the correct error-handling crate. `thiserror` and `anyhow` are not competitors: they serve opposite sides of the API boundary. `thiserror` builds **structured** error types whose callers can match on variants and recover. `anyhow` builds **opaque** error values whose callers only display the message and the source chain. Picking the wrong one leaks an unmatchable error type into a library API or buries a binary in pointless boilerplate.

Cross-references: [[rust-impl-error-handling]] (the `?` operator, `Result` vs `panic!`, custom error types from scratch), [[rust-errors-runtime]] (panics, `unwrap`/`expect`, `Box<dyn Error>`), [[rust-syntax-traits]] (the `std::error::Error` trait, `From`, derive macros), [[rust-impl-cargo-project]] (adding crate dependencies, library vs binary crates).

---

## When to use this skill

- The user asks "which error crate should I use" or "thiserror vs anyhow".
- A library crate needs a public error type whose callers must match on causes.
- A binary or application crate needs `fn main() -> Result<()>` and short error glue.
- An error message no longer carries its underlying cause (broken source chain).
- The user wants to recover a concrete error type out of an opaque `anyhow::Error`.
- A project mixes both crates and the boundary between them is unclear.
- The user wants to migrate code from `anyhow` to `thiserror` or the reverse.

---

## The one rule behind the choice

> `thiserror` is for errors the **caller acts on**. `anyhow` is for errors the **caller only reports**.

A library cannot know how its callers will handle failures, so it MUST expose a structured type. An application is the final caller, so it can collapse every failure into one opaque type. ALWAYS pick the crate from the **caller's** needs, never from convenience at the call site.

---

## Decision table

| Crate type | Use | Error crate | Public error type |
|------------|-----|-------------|-------------------|
| Library (`--lib`, published, reused) | callers match on variants and recover | `thiserror` | a `thiserror`-derived enum |
| Binary / application (`--bin`, `main`) | callers (humans) display message + chain | `anyhow` | `anyhow::Error` (internal only) |
| Library boundary inside a big app | internal modules expose structured errors | `thiserror` per module | module-local enum |
| App boundary (`main`, request handlers) | collect every module error, add context | `anyhow` | `anyhow::Error` |

ALWAYS use `thiserror` for library crates. NEVER expose `anyhow::Error` in a library's public API: the caller then cannot match on the failure, only print it.

ALWAYS use `anyhow` for the top-level error type of a binary. NEVER hand-write a `thiserror` enum for a binary's `main` return type when no caller ever matches on it: that is boilerplate with zero benefit.

---

## thiserror: structured library errors

`thiserror` is a derive macro. It generates `Display` and `std::error::Error` impls so the type behaves exactly as if those impls were hand-written. The crate itself never appears in the public API. Current version: `2.x` (`thiserror = "2"`).

### Minimal pattern

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("config file not found at {path}")]
    NotFound { path: String },

    #[error("config field `{0}` is missing")]
    MissingField(String),

    #[error("failed to read config file")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Parse(#[from] toml::de::Error),
}
```

`#[derive(Error, Debug)]` is mandatory: `std::error::Error` requires `Debug` as a supertrait, so the derive ALWAYS needs `Debug` next to `Error`.

### Attribute reference

| Attribute | Generates / does |
|-----------|------------------|
| `#[error("text {field}")]` | the `Display` impl, with `{field}` / `{0}` interpolation |
| `#[error("{0:?}")]` | interpolates with the `Debug` formatter instead of `Display` |
| `#[from]` | a `From<Inner>` impl AND marks that field as the source; field must be the only one |
| `#[source]` | marks a field as the underlying cause returned by `Error::source()`; no `From` |
| a field literally named `source` | treated as `#[source]` automatically |
| `#[error(transparent)]` | delegates `Display` and `source()` to the single wrapped error, adds no text |

`#[from]` implies `#[source]`. Use `#[from]` when the conversion should be automatic via `?`. Use `#[source]` when you build the variant manually but still want the cause chained. Use `#[error(transparent)]` for a pure pass-through variant (one inner error, no extra message).

### Why the source chain matters

The `?` operator calls `From::from` to convert errors. A `#[from]` variant lets a foreign error flow through `?` into your enum while the original error stays reachable via `Error::source()`. NEVER format the inner error into the message string instead. `#[error("io failed: {0}")]` with a plain field loses nothing, but `#[error("io failed")]` on a field that is NOT `#[source]`/`#[from]` discards the cause: tools that walk `.source()` see a dead end.

See `references/methods.md` for every attribute with examples and `references/examples.md` for full enum patterns.

---

## anyhow: opaque application errors

`anyhow` provides one dynamic error type that absorbs any error. Current version: `1.x` (`anyhow = "1"`).

### Minimal pattern

```rust
use anyhow::{Context, Result, bail, ensure};

fn load_settings(path: &str) -> Result<Settings> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading settings from {path}"))?;

    ensure!(!raw.is_empty(), "settings file {path} is empty");

    let settings: Settings = toml::from_str(&raw)
        .context("parsing settings as TOML")?;

    if settings.workers == 0 {
        bail!("settings.workers must be at least 1");
    }
    Ok(settings)
}

fn main() -> Result<()> {
    let settings = load_settings("config.toml")?;
    run(&settings)
}
```

### API reference

| Item | Purpose |
|------|---------|
| `anyhow::Result<T>` | alias for `Result<T, anyhow::Error>` |
| `anyhow::Error` | wraps any `E: std::error::Error + Send + Sync + 'static` |
| `.context("msg")` | attach a static-ish context message (eager) |
| `.with_context(\|\| ...)` | attach context built lazily, only on the error path |
| `bail!("msg")` | `return Err(anyhow!("msg"))` early |
| `ensure!(cond, "msg")` | return an error early if `cond` is false |
| `anyhow!("msg")` | construct an ad-hoc error value |
| `.downcast_ref::<T>()` | borrow the concrete error type back out, if it is `T` |

Any error with `Error + Send + Sync + 'static` converts into `anyhow::Error` automatically through `?`. ALWAYS propagate with `?` plus `.context(...)`, NEVER `.unwrap()` an `anyhow::Result`: `.unwrap()` panics and throws away the very context chain `anyhow` exists to build.

Use `.with_context` when the message needs `format!` or other work; use `.context` for a plain string. The lazy form avoids formatting on the success path.

### Recovering a concrete type with downcast

Even though `anyhow::Error` is opaque, the original typed error is still inside it:

```rust
fn handle(err: anyhow::Error) {
    if let Some(io) = err.downcast_ref::<std::io::Error>() {
        if io.kind() == std::io::ErrorKind::NotFound {
            // recover specifically from a missing file
        }
    }
}
```

`downcast_ref` is the escape hatch for the rare case an application must branch on a cause. If branching is common and structured, that is the signal the type belongs in a `thiserror` enum instead.

---

## Combining both crates

The standard layout of a real project uses both, each on its own side of the boundary:

1. **Each library crate / internal module** defines its own `thiserror` enum and returns `Result<T, ThatEnum>`. Variants carry `#[from]` for foreign errors so `?` composes cleanly.
2. **The application layer** (`main`, request handlers, job runners) returns `anyhow::Result<T>`. It calls the libraries, and every library enum converts into `anyhow::Error` through `?` because a `thiserror` enum implements `std::error::Error`.
3. The application adds human context with `.context(...)` as errors bubble up toward `main`.

```rust
// library crate `store`
#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error("record {id} not found")]
    NotFound { id: u64 },
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

// application crate
use anyhow::{Context, Result};

fn run() -> Result<()> {
    let user = store::fetch_user(42)            // Result<_, StoreError>
        .context("loading the active user")?;   // -> anyhow::Error here
    Ok(())
}
```

The library never mentions `anyhow`; the application never hand-rolls an error enum. See `references/examples.md` for a full multi-crate walkthrough.

---

## Migration paths

### anyhow to thiserror (a binary grows into a library)

Triggered when callers start needing to match on failures. Steps:

1. Enumerate every distinct failure the function can produce.
2. Define a `#[derive(Error, Debug)]` enum with one variant per failure.
3. Replace each `bail!`/`anyhow!` site with `return Err(MyError::Variant ...)`.
4. Replace `#[from]`-able foreign errors so `?` still composes.
5. Change the signature from `anyhow::Result<T>` to `Result<T, MyError>`.
6. Move any leftover `.context(...)` into a descriptive variant or a `#[source]` field.

### thiserror to anyhow (a layer becomes app-only)

Triggered when an enum's variants are never matched, only displayed. Steps:

1. Confirm no external caller matches on the enum (search for `match` on it).
2. Change the signature to `anyhow::Result<T>`.
3. Delete the enum; foreign errors already convert through `?`.
4. Re-add lost messages as `.context(...)` at each call site.

NEVER migrate a still-public library error type to `anyhow`: that breaks every downstream caller that matched on it.

---

## eyre and color-eyre (anyhow-family alternatives)

`eyre` is an `anyhow` fork with the same API (`eyre::Result`, `.wrap_err(...)` instead of `.context(...)`, `eyre!`, `bail!`, `ensure!`) but a **customizable report handler**. `color-eyre` plugs into `eyre` to render colorized, sectioned error reports with spans and suggestions, which is valued for CLI tools. Choose `eyre` + `color-eyre` over `anyhow` only when richer human-facing error reports justify the extra dependency. The decision rule is unchanged: still an **application-only** opaque error type, NEVER a library's public API.

---

## Anti-patterns

NEVER do these. Each is explained with the correct fix in `references/anti-patterns.md`.

1. **Exposing `anyhow::Error` in a library's public API.** Callers cannot match on the failure, only print it. Libraries MUST return a `thiserror` enum.
2. **Hand-rolling a `thiserror` enum for a binary's top-level error.** When no caller ever matches the variants, this is boilerplate with zero benefit; use `anyhow`.
3. **Losing the source chain.** Formatting an inner error into the message string instead of marking the field `#[source]` or `#[from]` makes `Error::source()` dead-end.
4. **`.unwrap()` on an `anyhow::Result`.** It panics and discards the context chain. Propagate with `?` plus `.context(...)`.
5. **Deriving `thiserror::Error` without `Debug`.** `std::error::Error` requires `Debug`; `#[derive(Error)]` alone fails to compile.
6. **Swallowing context by mapping to `String`.** `.map_err(|e| e.to_string())?` flattens a rich error into text and severs `source()`. Keep the typed error or wrap with `anyhow`.

---

## Reference files

- `references/methods.md` : every `thiserror` attribute and `anyhow` macro/method with signatures and behavior.
- `references/examples.md` : complete enum patterns, the combined library-plus-app layout, and migration before/after.
- `references/anti-patterns.md` : the six anti-patterns above, each with the failing code and the correct fix.

## Verified sources

- thiserror: https://docs.rs/thiserror/latest/thiserror/ (verified 2026-05-20, v2.x)
- anyhow: https://docs.rs/anyhow/latest/anyhow/ (verified 2026-05-20, v1.x)
- The Rust Book, ch. 9 Error Handling: https://doc.rust-lang.org/book/ch09-00-error-handling.html
- Rust API Guidelines (error types): https://rust-lang.github.io/api-guidelines/
