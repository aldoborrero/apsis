# Methods and Attributes Reference

Complete reference for the `thiserror` derive attributes and the `anyhow` API. Verified against https://docs.rs/thiserror/latest/thiserror/ and https://docs.rs/anyhow/latest/anyhow/ on 2026-05-20. `thiserror` 2.x, `anyhow` 1.x, Rust 1.85+, edition 2024.

---

## thiserror

`thiserror` is a derive macro that generates the `std::fmt::Display` and `std::error::Error` impls for an error type. The derived code is exactly what a hand-written impl would produce; `thiserror` itself never shows up in the public API or in trait bounds.

Add to `Cargo.toml`:

```toml
[dependencies]
thiserror = "2"
```

### `#[derive(Error, Debug)]`

Applied to an `enum`, a `struct` with named fields, a tuple `struct`, or a unit `struct`.

- `Error` generates the `Display` impl (from `#[error(...)]`) and the `std::error::Error` impl (`source()`, and `provide()` on nightly when a `Backtrace` field is present).
- `Debug` is mandatory and separate. `std::error::Error` has `Debug` as a supertrait, so the type ALWAYS needs `#[derive(..., Debug)]`. `#[derive(Error)]` without `Debug` fails to compile.

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MyError {
    #[error("not found")]
    NotFound,
}
```

### `#[error("...")]` : the Display message

Required on every enum variant and on a struct. Defines the `Display` output with field interpolation.

| Form | Expands to |
|------|------------|
| `#[error("text {field}")]` | `write!(f, "text {}", self.field)` for named fields |
| `#[error("text {0}")]` | `write!(f, "text {}", self.0)` for tuple fields by index |
| `#[error("{field:?}")]` | interpolates with the `Debug` formatter |
| `#[error("{0:?}")]` | interpolates a tuple field with `Debug` |
| `#[error("got {0}, max {max}", max = i32::MAX)]` | trailing `name = expr` arguments allow custom expressions |

Interpolation reads the variant's own fields. Standard format spec (`{value:>8}`, `{value:.2}`, etc.) is supported. The message SHOULD be lowercase and SHOULD NOT end with punctuation, per Rust API guidelines, because callers compose it into larger messages.

### `#[from]`

Placed on exactly one field of a variant. Generates a `From<ThatFieldType>` impl for the enclosing enum, and marks the field as the error source.

- The variant must contain **only** that field (a `Backtrace` field is the sole allowed companion).
- `#[from]` implies `#[source]`: the field is returned by `Error::source()`.
- Enables `?` to convert a foreign error into your enum automatically.

```rust
#[derive(Error, Debug)]
pub enum DataError {
    #[error("disk read failed")]
    Io(#[from] std::io::Error),       // From<std::io::Error> generated
    #[error("bad json")]
    Json(#[from] serde_json::Error),  // From<serde_json::Error> generated
}

fn load() -> Result<String, DataError> {
    let s = std::fs::read_to_string("f")?; // io::Error -> DataError::Io
    Ok(s)
}
```

NEVER put two `#[from]` attributes for the same source type on different variants: the generated `From` impls collide and the crate fails to compile.

### `#[source]`

Marks a field as the underlying cause without generating a `From` impl. Use it when the variant is constructed manually but the cause must still be chained for `Error::source()`.

```rust
#[derive(Error, Debug)]
pub enum RequestError {
    #[error("request to {url} failed")]
    Failed {
        url: String,
        #[source]
        cause: reqwest::Error,
    },
}
```

A field literally **named `source`** is treated as `#[source]` automatically; the explicit attribute is then optional but harmless.

### `#[error(transparent)]`

Placed on a variant or struct that wraps exactly one inner error and adds no message of its own. Delegates both `Display` and `source()` straight to the inner error.

```rust
#[derive(Error, Debug)]
pub enum AppError {
    #[error("config invalid")]
    Config(String),
    #[error(transparent)]               // no message text of its own
    Other(#[from] anyhow::Error),
}
```

Use `transparent` for a catch-all pass-through variant. It is the one idiomatic place a library may hold an `anyhow::Error`: as an opaque internal variant, never as the whole public type.

### `#[backtrace]`

Forwards `provide()` to the source so a backtrace propagates. A field of type `std::backtrace::Backtrace` is auto-detected. Backtrace provision requires a nightly toolchain feature; on stable, a `Backtrace` field still compiles but `provide()` is not generated.

---

## anyhow

`anyhow` provides a single dynamic error type for applications. It absorbs any error and builds a context chain, but is opaque: callers display it, they do not match on it.

Add to `Cargo.toml`:

```toml
[dependencies]
anyhow = "1"
```

### `anyhow::Result<T>`

```rust
pub type Result<T, E = anyhow::Error> = std::result::Result<T, E>;
```

Use it as the return type of application functions: `fn run() -> anyhow::Result<()>`. `fn main() -> anyhow::Result<()>` is valid and prints the error plus its chain on exit.

### `anyhow::Error`

A trait-object-based error type. It wraps any `E: std::error::Error + Send + Sync + 'static`. The `?` operator converts such errors into `anyhow::Error` automatically, so heterogeneous error types compose without a hand-written `From`.

Key methods on `anyhow::Error`:

| Method | Returns | Purpose |
|--------|---------|---------|
| `.context(C)` | `anyhow::Error` | wrap with an additional context message |
| `.chain()` | `Chain` iterator | iterate this error and every source under it |
| `.root_cause()` | `&dyn Error` | the deepest error in the chain |
| `.downcast_ref::<T>()` | `Option<&T>` | borrow the concrete error if it is `T` |
| `.downcast_mut::<T>()` | `Option<&mut T>` | mutably borrow the concrete error if it is `T` |
| `.downcast::<T>()` | `Result<T, anyhow::Error>` | take the concrete error by value, or give the error back |
| `.is::<T>()` | `bool` | whether the concrete error is `T` |

### The `Context` trait

Imported with `use anyhow::Context;`. Adds two methods to `Result<T, E>` and `Option<T>`:

- `.context(msg)` : attach a context value eagerly. Best for a plain string literal.
- `.with_context(|| msg)` : attach context built by a closure, evaluated only on the error path. Best when the message needs `format!` or other work.

```rust
use anyhow::Context;

let data = std::fs::read(path)
    .with_context(|| format!("reading data from {path}"))?;

let port: u16 = raw
    .parse()
    .context("PORT must be a number")?;
```

`.context` on `Option<T>` converts `None` into an error with that message, replacing a bare `.ok_or_else(...)`.

### `bail!`

```rust
bail!("message")
bail!("formatted {value}")
```

Equivalent to `return Err(anyhow!(...))`. Use it for an early exit on an unrecoverable condition mid-function.

### `ensure!`

```rust
ensure!(condition, "message")
ensure!(workers > 0, "workers must be at least {min}", min = 1)
```

If `condition` is `false`, returns early with the error. Like `assert!` but returns an `Err` instead of panicking. Use it for precondition checks.

### `anyhow!`

```rust
let e: anyhow::Error = anyhow!("ad-hoc error");
let e = anyhow!("code {code}", code = 42);
let e = anyhow!(some_other_error);   // wrap a non-anyhow error value
```

Constructs an `anyhow::Error` from a message or from an existing error value. `format_err!` is a re-export alias of `anyhow!`.

### Backtraces

`anyhow` captures a backtrace automatically (Rust >= 1.65) when `RUST_BACKTRACE=1` or `RUST_LIB_BACKTRACE=1` is set. No code change is needed; the backtrace prints with the error in `main`'s output.

---

## Quick mapping

| Need | thiserror | anyhow |
|------|-----------|--------|
| auto `From` for `?` | `#[from]` | built in for any `Error + Send + Sync + 'static` |
| record the cause | `#[source]` / `#[from]` / field named `source` | automatic, plus `.context()` layers |
| add a human message | `#[error("...")]` per variant | `.context()` / `.with_context()` |
| early-return an error | `return Err(MyError::Variant)` | `bail!` / `ensure!` |
| construct ad-hoc | a unit or string variant | `anyhow!` |
| recover a concrete type | already structured: `match` | `.downcast_ref::<T>()` |
