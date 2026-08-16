# examples.md: working error-handling code

All snippets are stand-alone and compile on Rust 1.85+, edition 2024 unless marked. They illustrate the patterns from `SKILL.md` end-to-end.

---

## 1. A library error: `enum` + `Display` + `Error` + `From`

```rust
use std::fmt;
use std::fs;
use std::io;
use std::num::ParseIntError;

#[derive(Debug)]
pub enum LoadError {
    Io { path: String, source: io::Error },
    Parse { path: String, source: ParseIntError },
    Empty(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io { path, .. }    => write!(f, "i/o error reading {}", path),
            LoadError::Parse { path, .. } => write!(f, "parse error in {}", path),
            LoadError::Empty(path)        => write!(f, "{} is empty", path),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoadError::Io { source, .. }    => Some(source),
            LoadError::Parse { source, .. } => Some(source),
            LoadError::Empty(_)             => None,
        }
    }
}

// NO blanket `From<io::Error>`: we need the path for context. Use map_err.
pub fn load_number(path: &str) -> Result<i32, LoadError> {
    let text = fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.into(),
        source,
    })?;
    if text.trim().is_empty() {
        return Err(LoadError::Empty(path.into()));
    }
    text.trim().parse::<i32>().map_err(|source| LoadError::Parse {
        path: path.into(),
        source,
    })
}
```

Key points:

- Each variant carries `source` for `Error::source()`.
- We use `map_err` (not `?` + `From`) because we need to attach the path. Where context is not needed, an `impl From<...>` is cleaner.
- `Display` is a one-liner; the source chain prints the cause.

---

## 2. Custom error: `From` impls + `?` chain

When the caller does not need extra context, blanket `From` impls keep the function body free of `map_err`:

```rust
use std::fs;
use std::io;
use std::num::ParseIntError;

#[derive(Debug)]
pub enum AppError {
    Io(io::Error),
    Parse(ParseIntError),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Io(e)    => write!(f, "i/o error: {}", e),
            AppError::Parse(e) => write!(f, "parse error: {}", e),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::Io(e)    => Some(e),
            AppError::Parse(e) => Some(e),
        }
    }
}

impl From<io::Error> for AppError {
    fn from(e: io::Error) -> Self { AppError::Io(e) }
}
impl From<ParseIntError> for AppError {
    fn from(e: ParseIntError) -> Self { AppError::Parse(e) }
}

pub fn first_int(path: &str) -> Result<i32, AppError> {
    let s = fs::read_to_string(path)?;        // io::Error -> AppError::Io
    let n: i32 = s.trim().parse()?;           // ParseIntError -> AppError::Parse
    Ok(n)
}
```

The `?` operator handles the conversion via the two `From` impls. Adding a new inner error type means adding one `From` impl, not editing every function.

---

## 3. Walking the source chain

Print the full cause chain of any error:

```rust
fn print_chain(e: &dyn std::error::Error) {
    eprintln!("error: {e}");
    let mut cur = e.source();
    while let Some(src) = cur {
        eprintln!("caused by: {src}");
        cur = src.source();
    }
}

fn main() {
    if let Err(e) = first_int("config.txt") {
        print_chain(&e);
        std::process::exit(1);
    }
}
```

Sample output:

```
error: i/o error: No such file or directory (os error 2)
caused by: No such file or directory (os error 2)
```

The duplication comes from `Display` of `AppError::Io` repeating the inner error. To avoid it, write `write!(f, "i/o error")` without the inner `{e}` and rely on `source()`.

---

## 4. `Result` combinators in a real pipeline

```rust
use std::num::ParseIntError;

fn parse_pair(s: &str) -> Result<(i32, i32), String> {
    let (a, b) = s.split_once(',').ok_or_else(|| format!("missing comma in {s:?}"))?;
    let a: i32 = a.trim().parse().map_err(|e: ParseIntError| e.to_string())?;
    let b: i32 = b.trim().parse().map_err(|e: ParseIntError| e.to_string())?;
    Ok((a, b))
}

fn sum_pair(s: &str) -> Result<i32, String> {
    parse_pair(s).map(|(a, b)| a + b)
}

fn sum_pair_or_zero(s: &str) -> i32 {
    sum_pair(s).unwrap_or(0)
}

fn sum_pair_with_fallback(s: &str) -> i32 {
    sum_pair(s).unwrap_or_else(|e| {
        eprintln!("warning: {e}, using 0");
        0
    })
}
```

Note the combinator chain: `.ok_or_else(...)` lazily builds the error message; `.map_err(...)` converts `ParseIntError` to `String` before `?`; `.map(...)` transforms the `Ok` value; `.unwrap_or_else(...)` recovers in the caller.

---

## 5. `no_std`: `core::error::Error` (Rust 1.81+)

```rust
#![no_std]
extern crate alloc;

use alloc::string::String;
use core::error::Error;
use core::fmt;

#[derive(Debug)]
pub struct ProtocolError {
    pub code: u32,
    pub message: String,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "protocol error {}: {}", self.code, self.message)
    }
}

impl Error for ProtocolError {}

pub fn parse_frame(buf: &[u8]) -> Result<u16, ProtocolError> {
    if buf.len() < 2 {
        return Err(ProtocolError {
            code: 1,
            message: String::from("buffer too short"),
        });
    }
    Ok(u16::from_le_bytes([buf[0], buf[1]]))
}
```

In `no_std` crates you MUST use `core::error::Error` (and `core::fmt`, `alloc::string::String`). The trait shape is identical to `std::error::Error`, which is now a re-export.

---

## 6. Panic vs Result: the same task, two endings

Library function: returns `Result` for an expected failure (file missing).

```rust
pub fn read_threshold(path: &str) -> Result<u32, AppError> {
    let s = std::fs::read_to_string(path)?;
    let n: u32 = s.trim().parse()?;
    Ok(n)
}
```

Application `main`: panics for a startup-only invariant.

```rust
fn main() {
    let threshold = read_threshold("threshold.txt")
        .expect("threshold.txt is required at startup");
    println!("threshold = {threshold}");
}
```

Inside the library: `assert!` / `debug_assert!` / `unreachable!()` for invariant violations.

```rust
fn binary_search_normalized(haystack: &[u32], needle: u32) -> Option<usize> {
    debug_assert!(haystack.windows(2).all(|w| w[0] <= w[1]),
                  "haystack must be sorted");
    haystack.binary_search(&needle).ok()
}
```

The `debug_assert!` is checked only in debug builds; in release it is compiled out. The contract is documented in the panic message, so failures point to the calling bug.

---

## 7. Converting `Option` to `Result` for `?` interop

A function returns `Result`, but one step yields an `Option`:

```rust
fn lookup_uppercase(map: &std::collections::HashMap<String, String>, key: &str)
    -> Result<String, String>
{
    let v = map.get(key).ok_or_else(|| format!("missing key {key:?}"))?;
    Ok(v.to_uppercase())
}
```

NEVER reach for `match opt { Some(x) => x, None => return Err(...) }`: that is `ok_or(...)?`.

---

## 8. Wrapping a third-party error with context

When you want to wrap an error from a dependency (e.g. `serde_json::Error`) into your own type while preserving the cause:

```rust
use std::fmt;

#[derive(Debug)]
pub struct ConfigError {
    pub path: String,
    pub source: serde_json::Error,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to parse config {}", self.path)
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub fn load_config(path: &str) -> Result<serde_json::Value, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError {
        path: path.into(),
        source: serde_json::Error::io(e),  // serde_json wraps io::Error
    })?;
    serde_json::from_str(&text).map_err(|source| ConfigError {
        path: path.into(),
        source,
    })
}
```

The `source` field carries the cause; `Display` adds the contextual layer. A walker prints both layers in order.

For an ergonomic `thiserror` rewrite, see [[rust-errors-thiserror-anyhow]].

---

## 9. `main() -> Result<(), Box<dyn Error>>` for quick scripts

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let n = read_threshold("threshold.txt")?;
    println!("threshold = {n}");
    Ok(())
}
```

The compiler accepts any error that satisfies the trait object bound. Use only in binary `main()` or test code. NEVER expose this signature from a library function (callers cannot match on variants).

---

## Sources

- [`std::result` examples](https://doc.rust-lang.org/std/result/#examples)
- [`std::error::Error`](https://doc.rust-lang.org/std/error/trait.Error.html)
- [`core::error::Error`](https://doc.rust-lang.org/core/error/trait.Error.html)
- [Rust Book Ch 9.2 "Recoverable Errors with `Result`"](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html)
- [Rust Book Ch 9.3 "To `panic!` or Not to `panic!`"](https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html)
- [Rust 1.81 release notes](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/)
