# Anti-patterns

The six recurring mistakes when choosing and applying `thiserror` and `anyhow`. Each entry shows the failing code, why it is wrong, and the correct fix. Rust 1.85+, edition 2024, `thiserror = "2"`, `anyhow = "1"`.

---

## 1. Exposing `anyhow::Error` in a library's public API

NEVER return `anyhow::Result` (or `anyhow::Error`) from a public function of a library crate.

### Wrong

```rust
// crate `parser` (a library)
use anyhow::Result;

pub fn parse(input: &str) -> Result<Ast> {  // anyhow::Error leaks out
    /* ... */
}
```

A downstream caller receives an opaque `anyhow::Error`. It cannot tell a syntax error from an IO error from an out-of-memory condition, so it cannot recover from any of them. It can only print the message. The library has forced every caller into report-only handling.

### Correct

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("unexpected token at byte {pos}")]
    UnexpectedToken { pos: usize },
    #[error("unterminated string starting at byte {pos}")]
    UnterminatedString { pos: usize },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn parse(input: &str) -> Result<Ast, ParseError> {
    /* ... */
}
```

A library cannot know how callers will handle failures, so it MUST expose a structured, matchable type. `anyhow` is for the final application, not for a reusable crate.

---

## 2. Hand-rolling a thiserror enum for a binary's top-level error

NEVER write a `thiserror` enum for the error type of a binary's `main` when no code ever matches its variants.

### Wrong

```rust
// src/main.rs of a CLI tool
#[derive(thiserror::Error, Debug)]
enum CliError {
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("config error")]
    Config(#[from] toml::de::Error),
    #[error("network error")]
    Net(#[from] reqwest::Error),
    // ... one variant per dependency, forever
}

fn main() -> Result<(), CliError> { /* ... */ }
```

`main` is the final caller. Nothing downstream will ever `match` on `CliError`. Every variant exists only to be displayed, so the enum is pure boilerplate that grows with every new dependency and adds zero capability.

### Correct

```rust
use anyhow::{Context, Result};

fn main() -> Result<()> {
    let cfg = load_config().context("loading configuration")?;
    run(&cfg).context("running the tool")?;
    Ok(())
}
```

`anyhow::Error` absorbs every error type automatically and `.context(...)` supplies human messages. Use `thiserror` only where a caller matches on variants.

---

## 3. Losing the source chain

NEVER format an inner error into the message string instead of marking the field a source.

### Wrong

```rust
#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    // the io::Error is baked into text; the field is NOT a source
    #[error("failed to load: {0}")]
    Io(String),
}

fn load() -> Result<(), LoadError> {
    std::fs::read("f").map_err(|e| LoadError::Io(e.to_string()))?;
    Ok(())
}
```

`LoadError::Io` holds a `String`. `Error::source()` returns `None`, so any tool that walks the cause chain (a logger, `anyhow`, an error reporter) hits a dead end. The structured `std::io::Error` (with its `ErrorKind`) is destroyed.

### Correct

```rust
#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("failed to load file")]
    Io(#[from] std::io::Error),  // #[from] keeps the io::Error as source()
}

fn load() -> Result<(), LoadError> {
    std::fs::read("f")?;  // converts via the generated From, chain intact
    Ok(())
}
```

Use `#[from]` (auto conversion) or `#[source]` (manual construction). The inner error stays reachable through `Error::source()` and keeps its type.

---

## 4. `.unwrap()` on an anyhow::Result

NEVER call `.unwrap()` (or `.expect()`) on an `anyhow::Result` in normal control flow.

### Wrong

```rust
fn run() -> anyhow::Result<()> {
    let raw = std::fs::read_to_string("config.toml").unwrap(); // panics, no context
    let cfg: Config = toml::from_str(&raw).unwrap();           // panics, no context
    Ok(())
}
```

`.unwrap()` panics on error and discards the context chain that `anyhow` exists to build. The user sees a raw panic and a backtrace into `unwrap`, not "failed to read config.toml: No such file or directory".

### Correct

```rust
use anyhow::{Context, Result};

fn run() -> Result<()> {
    let raw = std::fs::read_to_string("config.toml")
        .context("reading config.toml")?;
    let cfg: Config = toml::from_str(&raw)
        .context("parsing config.toml")?;
    Ok(())
}
```

Propagate with `?` and attach `.context(...)`. Reserve `.unwrap()`/`.expect()` for genuine programmer-invariant violations, not for operational failures.

---

## 5. Deriving thiserror::Error without Debug

NEVER write `#[derive(Error)]` alone. The derive does not imply `Debug`.

### Wrong

```rust
use thiserror::Error;

#[derive(Error)]            // missing Debug
pub enum MyError {
    #[error("boom")]
    Boom,
}
```

This fails to compile. `std::error::Error` declares `Debug` as a supertrait, so any type implementing `Error` must also implement `Debug`. The compiler reports `the trait bound MyError: Debug is not satisfied`.

### Correct

```rust
use thiserror::Error;

#[derive(Error, Debug)]     // Debug alongside Error, always
pub enum MyError {
    #[error("boom")]
    Boom,
}
```

ALWAYS pair `Error` with `Debug` in the derive list. `#[derive(Error, Debug)]` is the canonical form.

---

## 6. Swallowing context by mapping to a String

NEVER flatten a typed error into a `String` with `.map_err(|e| e.to_string())`.

### Wrong

```rust
fn load() -> Result<Config, String> {
    let raw = std::fs::read_to_string("config.toml")
        .map_err(|e| e.to_string())?;        // io::Error -> String, type lost
    let cfg = toml::from_str(&raw)
        .map_err(|e| e.to_string())?;        // toml error -> String, source lost
    Ok(cfg)
}
```

`String` is not a real error type. `Error::source()` is gone, the `ErrorKind` is gone, downcasting is impossible, and `?` no longer composes with anything. Callers receive a flat string and can do nothing but print it.

### Correct, application side

```rust
use anyhow::{Context, Result};

fn load() -> Result<Config> {
    let raw = std::fs::read_to_string("config.toml")
        .context("reading config.toml")?;
    let cfg = toml::from_str(&raw)
        .context("parsing config.toml")?;
    Ok(cfg)
}
```

### Correct, library side

```rust
#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("reading config file")]
    Io(#[from] std::io::Error),
    #[error("parsing config file")]
    Parse(#[from] toml::de::Error),
}

fn load() -> Result<Config, ConfigError> {
    let raw = std::fs::read_to_string("config.toml")?;
    let cfg = toml::from_str(&raw)?;
    Ok(cfg)
}
```

Keep the typed error. Wrap it with `anyhow` in an application, or with a `thiserror` enum in a library. Never reduce it to text.
