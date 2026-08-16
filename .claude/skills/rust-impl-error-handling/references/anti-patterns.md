# anti-patterns.md: error-handling mistakes and how to fix them

Each entry: NAME, WHY it is wrong, FIX. Apply the fix; do not paper over the symptom.

---

## AP-1: `.unwrap()` in library code

```rust
pub fn parse(s: &str) -> i32 {
    s.trim().parse().unwrap()   // BAD: panics on bad input
}
```

WHY: a recoverable parse failure becomes a process-killing panic. Callers cannot intercept it. Libraries that panic on bad input are unusable in production (any caller error tears the process down).

FIX: return `Result<i32, E>` and propagate.

```rust
pub fn parse(s: &str) -> Result<i32, std::num::ParseIntError> {
    s.trim().parse()
}
```

Use `.unwrap()` only in tests, examples, prototypes, and at the top of `main()` for startup-only invariants.

---

## AP-2: `.expect("...")` for runtime errors

```rust
let s = std::fs::read_to_string(path).expect("config file");
```

WHY: `expect` is `unwrap` with a message. The message documents a *programmer-asserted invariant*, not an end-user error. A missing config file is a user error, not an invariant violation.

FIX: return `Result` and let the application decide how to report it.

```rust
let s = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
    path: path.into(),
    source: e,
})?;
```

`expect` is appropriate for things like `std::env::var("HOME").expect("HOME is unset, cannot proceed")` at the very top of `main()`.

---

## AP-3: Error type without `Display` + `Error`

```rust
#[derive(Debug)]
pub struct MyError(String);
```

WHY: any framework that walks errors (anyhow, eyre, `Box<dyn Error>`, the `?` operator across a different error type) requires `Display` and `std::error::Error`. Without them, `MyError` cannot compose; you write boilerplate at every call site.

FIX: implement both (or derive via thiserror).

```rust
impl std::fmt::Display for MyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for MyError {}
```

---

## AP-4: Forgetting `From<InnerError>`

```rust
fn load() -> Result<i32, MyError> {
    let s = std::fs::read_to_string("x")?;    // ERROR E0277
    Ok(s.trim().parse()?)                     // ERROR E0277
}
```

WHY: `?` calls `From::from(err)`. Without `impl From<io::Error> for MyError` and `impl From<ParseIntError> for MyError` the compiler bails out.

FIX: one `From` impl per inner error type you propagate, OR `map_err` to convert inline.

```rust
impl From<std::io::Error>        for MyError { fn from(e: std::io::Error)        -> Self { MyError::Io(e) } }
impl From<std::num::ParseIntError> for MyError { fn from(e: std::num::ParseIntError) -> Self { MyError::Parse(e) } }
```

NEVER write a `match` ladder by hand; `From` + `?` is the right pattern.

---

## AP-5: Returning `Box<dyn Error>` from a library function

```rust
// In a library crate:
pub fn load() -> Result<Config, Box<dyn std::error::Error>> { /* ... */ }
```

WHY: callers cannot match on specific variants, cannot downcast safely (the bound is loose), and cannot react to recoverable kinds. The error type becomes opaque forever, and you have built `anyhow::Error` by hand.

FIX: a concrete `enum` error (often via `thiserror`).

```rust
pub fn load() -> Result<Config, LoadError> { /* ... */ }

#[derive(Debug)]
pub enum LoadError { Io(std::io::Error), Parse(serde_json::Error), MissingField(String) }
```

`Box<dyn Error>` is acceptable in `fn main()` and inside a single error variant that captures a heterogeneous cause.

---

## AP-6: `Result<(), ()>`

```rust
pub fn validate(input: &str) -> Result<(), ()> {
    if input.is_empty() { Err(()) } else { Ok(()) }
}
```

WHY: the `Err(())` value carries zero information. Callers know it failed but not why. Logging a chain printer produces nothing useful.

FIX: a real error type, even a unit struct with a meaningful name.

```rust
#[derive(Debug)]
pub struct EmptyInput;
impl std::fmt::Display for EmptyInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("input must not be empty")
    }
}
impl std::error::Error for EmptyInput {}

pub fn validate(input: &str) -> Result<(), EmptyInput> {
    if input.is_empty() { Err(EmptyInput) } else { Ok(()) }
}
```

---

## AP-7: `panic!` for expected failures

```rust
let port: u16 = std::env::var("PORT").unwrap().parse().unwrap();
```

WHY: missing or malformed `PORT` is an end-user / deployment error, not an invariant violation. Panicking kills the process with no recovery path; in a long-running service, with no remediation message.

FIX: surface the error in the return type.

```rust
fn read_port() -> Result<u16, ConfigError> {
    let raw = std::env::var("PORT").map_err(|e| ConfigError::EnvMissing("PORT", e))?;
    raw.parse().map_err(|e| ConfigError::EnvParse("PORT", e))
}
```

Panic mechanics (unwind, abort, panic hooks) are covered in [[rust-errors-runtime]].

---

## AP-8: Duplicating the cause in `Display`

```rust
impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to load config: {}", self.source)   // BAD
    }
}
impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { Some(&self.source) }
}
```

WHY: error report tools walk `source()` and print each layer. With this `Display`, the cause is printed once via `Display` and again via the chain walker. Users see duplicate text.

FIX: keep `Display` to *this layer's* message; let the chain walker print the cause.

```rust
write!(f, "failed to load config {}", self.path)
```

---

## AP-9: `std::error::Error` in a `no_std` crate

```rust
#![no_std]
use std::error::Error;   // ERROR: unresolved import
```

WHY: in `#![no_std]` the `std` crate is unavailable. Before Rust 1.81 there was no `Error` trait in `core`, so `no_std` libraries reinvented one.

FIX: since 1.81 (2024-09-05), use `core::error::Error`. The trait is identical and `std::error::Error` is now a re-export.

```rust
#![no_std]
use core::error::Error;
```

---

## AP-10: `.ok()` to swallow `Err` in library code

```rust
let val = load_value().ok().unwrap_or_default();
```

WHY: silently discards the cause. The caller can no longer log it, retry it, or report it. Bugs hide for months until they cause symptoms far from the source.

FIX: propagate via `?` or transform via `.map_err`; if a fallback is legitimately desired, use `.unwrap_or_else(|e| { log::warn!("{e}"); default })` so the error is at least observed.

---

## AP-11: Mixing `anyhow::Error` into a library's public API

```rust
// In a library crate:
pub fn fetch_user(id: u64) -> anyhow::Result<User> { /* ... */ }
```

WHY: `anyhow::Error` is an opaque wrapper. Library callers cannot match on variants, cannot react differently to "not found" vs "permission denied" vs "network". Application binaries lose all variant information at the library boundary.

FIX: return a concrete error type from libraries; let binaries convert with `?` into `anyhow::Error` if they want application-level glue.

```rust
// library
pub fn fetch_user(id: u64) -> Result<User, FetchError> { /* ... */ }

// binary
fn main() -> anyhow::Result<()> {
    let u = fetch_user(42).context("fetching user 42")?;
    println!("{u:?}");
    Ok(())
}
```

See [[rust-errors-thiserror-anyhow]] for the full split.

---

## AP-12: Holding a borrow / lock and panicking inside `?`

```rust
let mut v = state.lock().unwrap();
let s = std::fs::read_to_string(&v.path)?;   // ? may early-return
v.text = s;
```

WHY: `?` may early-return with a poisoned lock still held. If the inner error converts via `From`, the `MutexGuard` is dropped on the way out (fine), but if the failure path panics, the lock is poisoned for every other thread.

FIX: limit the scope of held locks; do I/O before acquiring the lock. (Combined with [[rust-impl-concurrency]] guidance.)

```rust
let s = std::fs::read_to_string(&path)?;     // I/O outside the lock
let mut v = state.lock().unwrap();
v.text = s;
```

---

## AP-13: `Result<T, anyhow::Error>` from `#[test]`

```rust
#[test]
fn it_works() -> anyhow::Result<()> { /* ... */ }
```

This is **not** an anti-pattern; tests are application-shaped and `anyhow::Result` is perfect there. Listed for clarity: do not refactor it into a library-style enum.

---

## AP-14: Forgetting `Send + Sync` on error types crossing async or thread boundaries

```rust
struct MyError(std::rc::Rc<str>);     // Rc: !Send
```

WHY: tasks spawned with `tokio::spawn` require `Future: Send`; an error type with `Rc` or `RefCell` makes the future `!Send`, and the compiler refuses the spawn.

FIX: use `Arc<str>` / `Arc<...>` / owned `String` for payloads in error types. Add the bounds explicitly in the API:

```rust
pub fn do_async() -> impl Future<Output = Result<(), MyError>> + Send { /* ... */ }
```

When wrapping causes, always use `Box<dyn std::error::Error + Send + Sync + 'static>`.

---

## AP-15: `unwrap_err()` to extract `Err` after checking `is_err()`

```rust
if r.is_err() {
    let e = r.unwrap_err();   // BAD: separate branch, double match
    log::error!("{e}");
}
```

WHY: two passes over the same `Result`; the `is_err`/`unwrap_err` pattern is `if let Err(e) = r { ... }` written badly.

FIX:

```rust
if let Err(e) = r {
    log::error!("{e}");
}
```

Or, when you want the value too: `match r { Ok(v) => ..., Err(e) => ... }`.

---

## Sources

- [Rust Book Ch 9](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
- [`std::error::Error`](https://doc.rust-lang.org/std/error/trait.Error.html)
- [`core::error::Error`](https://doc.rust-lang.org/core/error/trait.Error.html)
- [Rust Error Index (E0277, etc.)](https://doc.rust-lang.org/error_codes/error-index.html)
- [Rust 1.81 release notes](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/)
- Anti-patterns observed in Rust GitHub issues and `clippy` lints (`unwrap_used`, `expect_used`, `panic`).
