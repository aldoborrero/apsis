# Examples

Complete, compilable patterns for `thiserror` and `anyhow`. All code targets Rust 1.85+, edition 2024, `thiserror = "2"`, `anyhow = "1"`.

---

## 1. A full thiserror library error enum

A library crate `imgproc` exposes one structured error type. Callers can match on every variant.

```rust
// src/error.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ImgError {
    /// File could not be opened. The io::Error is the chained source.
    #[error("could not open image file `{path}`")]
    Open {
        path: String,
        #[source]
        cause: std::io::Error,
    },

    /// A foreign decode error converted automatically through `?`.
    #[error("failed to decode image")]
    Decode(#[from] image::ImageError),

    /// A pure pass-through wrapper, no message of its own.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// A plain validated condition with no underlying cause.
    #[error("image is {width}x{height}, expected at most {max}px on a side")]
    TooLarge { width: u32, height: u32, max: u32 },
}
```

```rust
// src/lib.rs
pub use error::ImgError;
mod error;

pub fn load(path: &str) -> Result<image::DynamicImage, ImgError> {
    let bytes = std::fs::read(path).map_err(|cause| ImgError::Open {
        path: path.to_string(),
        cause,
    })?;
    let img = image::load_from_memory(&bytes)?; // ImageError -> ImgError::Decode
    let (w, h) = (img.width(), img.height());
    if w > 8192 || h > 8192 {
        return Err(ImgError::TooLarge { width: w, height: h, max: 8192 });
    }
    Ok(img)
}
```

A caller matches and recovers:

```rust
match imgproc::load("photo.png") {
    Ok(img) => use_image(img),
    Err(imgproc::ImgError::TooLarge { .. }) => downscale_and_retry(),
    Err(imgproc::ImgError::Open { path, .. }) => prompt_for_path(&path),
    Err(other) => report(other),
}
```

The `Open` variant builds manually, so it uses `#[source]`. The `Decode` and `Io` variants convert automatically, so they use `#[from]`. The cause stays reachable through `Error::source()` in every case.

---

## 2. A full anyhow application

A binary crate. Every function returns `anyhow::Result`; `main` prints the error chain on exit.

```rust
// src/main.rs
use anyhow::{Context, Result, anyhow, bail, ensure};

fn main() -> Result<()> {
    let cfg = load_config("app.toml").context("starting up")?;
    serve(&cfg)
}

fn load_config(path: &str) -> Result<Config> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading config from {path}"))?;

    ensure!(!raw.trim().is_empty(), "config file {path} is empty");

    let cfg: Config = toml::from_str(&raw)
        .with_context(|| format!("parsing {path} as TOML"))?;

    if cfg.port == 0 {
        bail!("config.port must not be 0");
    }
    Ok(cfg)
}

fn serve(cfg: &Config) -> Result<()> {
    let listener = bind(cfg.port)
        .with_context(|| format!("binding to port {}", cfg.port))?;
    if cfg.workers == 0 {
        return Err(anyhow!("at least one worker is required"));
    }
    Ok(())
}
```

On failure the program prints the message plus every `.context` layer and the original source, for example:

```
Error: starting up

Caused by:
    0: reading config from app.toml
    1: No such file or directory (os error 2)
```

`std::io::Error`, `toml::de::Error`, and the ad-hoc `anyhow!` errors all flow into `anyhow::Result` through `?` with no hand-written `From`.

---

## 3. Combining thiserror and anyhow

Library modules return structured errors; the application layer collects them under `anyhow`.

```rust
// crate `store` (library) : structured, public, matchable
#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error("user {id} not found")]
    NotFound { id: u64 },
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

pub fn fetch_user(id: u64) -> Result<User, StoreError> {
    /* ... */
}
```

```rust
// crate `auth` (library) : its own structured error
#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("token expired")]
    Expired,
    #[error("signature mismatch")]
    BadSignature,
}

pub fn verify(token: &str) -> Result<Claims, AuthError> {
    /* ... */
}
```

```rust
// crate `server` (application) : opaque, anyhow at the boundary
use anyhow::{Context, Result};

fn handle_request(token: &str, user_id: u64) -> Result<Response> {
    let claims = auth::verify(token)              // Result<_, AuthError>
        .context("verifying the request token")?; // -> anyhow::Error

    let user = store::fetch_user(user_id)         // Result<_, StoreError>
        .with_context(|| format!("loading user {user_id}"))?;

    Ok(render(&claims, &user))
}
```

Both `StoreError` and `AuthError` implement `std::error::Error`, so `?` converts each into `anyhow::Error`. Neither library mentions `anyhow`; the application never hand-rolls an enum. This is the standard division of labor.

### Recovering a concrete type at the boundary

The application can still branch on a specific library error via `downcast_ref`:

```rust
fn handle(err: anyhow::Error) -> Response {
    if let Some(store::StoreError::NotFound { id }) =
        err.downcast_ref::<store::StoreError>()
    {
        return Response::not_found(*id);
    }
    Response::internal_error(&err)
}
```

Use `downcast_ref` for the occasional special case. If the application branches on causes constantly, that logic belongs in a `thiserror` enum instead.

---

## 4. Migration: anyhow to thiserror

A function started as application glue but is now reused by callers that must match on its failures.

### Before (anyhow)

```rust
use anyhow::{Context, Result, bail};

pub fn parse_record(line: &str) -> Result<Record> {
    let (key, value) = line
        .split_once('=')
        .context("record line missing `=` separator")?;
    if key.is_empty() {
        bail!("record key is empty");
    }
    let value: i64 = value
        .trim()
        .parse()
        .with_context(|| format!("record value `{value}` is not an integer"))?;
    Ok(Record { key: key.to_string(), value })
}
```

### After (thiserror)

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RecordError {
    #[error("record line missing `=` separator")]
    NoSeparator,
    #[error("record key is empty")]
    EmptyKey,
    #[error("record value `{value}` is not an integer")]
    BadValue {
        value: String,
        #[source]
        cause: std::num::ParseIntError,
    },
}

pub fn parse_record(line: &str) -> Result<Record, RecordError> {
    let (key, value) = line.split_once('=').ok_or(RecordError::NoSeparator)?;
    if key.is_empty() {
        return Err(RecordError::EmptyKey);
    }
    let value: i64 = value.trim().parse().map_err(|cause| RecordError::BadValue {
        value: value.trim().to_string(),
        cause,
    })?;
    Ok(Record { key: key.to_string(), value })
}
```

Each `bail!`/`context` site became a named variant. The `parse` error is preserved as a `#[source]` field instead of being flattened into a string. Callers can now `match` on `RecordError`.

---

## 5. Migration: thiserror to anyhow

An internal helper had a `thiserror` enum, but no caller ever matched its variants; they only displayed it.

### Before (thiserror)

```rust
#[derive(thiserror::Error, Debug)]
enum CacheError {
    #[error("cache directory unavailable")]
    Dir(#[from] std::io::Error),
    #[error("cache entry corrupt")]
    Corrupt(#[from] serde_json::Error),
}

fn read_cache(path: &str) -> Result<Entry, CacheError> {
    let raw = std::fs::read_to_string(path)?;
    let entry = serde_json::from_str(&raw)?;
    Ok(entry)
}
```

### After (anyhow)

```rust
use anyhow::{Context, Result};

fn read_cache(path: &str) -> Result<Entry> {
    let raw = std::fs::read_to_string(path)
        .context("cache directory unavailable")?;
    let entry = serde_json::from_str(&raw)
        .context("cache entry corrupt")?;
    Ok(entry)
}
```

The enum is gone; foreign errors still convert through `?`. The variant messages became `.context(...)` strings. Do this ONLY when the type is internal: never collapse a still-public library error type, as that breaks downstream `match` arms.
