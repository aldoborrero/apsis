---
name: rust-impl-error-handling
description: >
  Use when the user designs Result-returning APIs, picks between Result and panic, uses the `?` operator with From conversion, defines custom error types, or considers core::error::Error (1.81) vs std::error::Error.
  Prevents `.unwrap()` in library code, hidden panics in `?` chains via wrong From impl, mixing thiserror and anyhow in library context, and forgetting source chain for error context.
  Covers: Result<T, E> vs Option<T>, `?` operator + From conversion chain, custom enum / struct error types, From impls for error conversion, the Error trait (std::error::Error, core::error::Error since 1.81 for no_std), source-chain pattern, panic vs Result decision, Result combinators (map / map_err / and_then / or_else / ok_or).
  Keywords: Result, Option, "? operator", From, "error conversion", "Error trait", "std::error::Error", "core::error::Error", "1.81 core error", "custom error", "error type", "thiserror vs anyhow", "library error", "panic vs Result", "map_err", "and_then", "or_else", "ok_or", "source chain", ".unwrap()", ".expect()", "error context", "Box<dyn Error>".
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# rust-impl-error-handling

Recoverable errors are values: `Result<T, E>` flows through the call graph, the `?` operator early-returns on `Err` with an automatic `From` conversion, and a well-designed error type implements `Display` + `std::error::Error` (or `core::error::Error` in `no_std`). Panic is reserved for *programmer bugs*, not for expected failure modes.

Cross-references: [[rust-errors-thiserror-anyhow]] (library vs application error-crate choice), [[rust-errors-runtime]] (panic mechanics, unwind vs abort), [[rust-errors-build-link]] (compile-time error decoding), [[rust-syntax-pattern-matching]] (matching on `Result` and `Option`).

---

## When to use this skill

- User writes `.unwrap()` or `.expect()` in library code.
- User asks "should this return `Result` or `Option`".
- User asks "why does `?` not compile" or gets `E0277: the trait From<X> for Y is not implemented`.
- User designs a new error type and chooses between `enum` and `struct`.
- User asks about `core::error::Error` vs `std::error::Error`.
- User wants to chain errors with a `source` (cause) pointer.
- User asks "should I panic here, or return `Result`".
- User looks for `Result` combinators (`map`, `map_err`, `and_then`, `or_else`, `ok_or`).

---

## Result vs Option: the rule

| Type | Models | Use when |
|------|--------|----------|
| `Option<T>` | Presence (`Some`) or absence (`None`) of a value. No failure semantics. | Lookup that may legitimately yield nothing: `HashMap::get`, `Vec::first`, optional config. |
| `Result<T, E>` | Success (`Ok`) or failure (`Err(E)`) of an operation, where `E` describes *why* it failed. | Anything that can fail with a *reason*: I/O, parsing, validation, network. |

ALWAYS return `Result<T, E>` when the caller needs to know *why* the operation failed. NEVER return `Option<T>` to mask an error: the caller loses the cause and cannot recover intelligently.

Convert between them when bridging APIs:

```rust
let v: Option<i32> = Some(7);
let r: Result<i32, &str> = v.ok_or("missing");          // Option -> Result
let r2: Result<i32, &str> = v.ok_or_else(|| "missing"); // lazy variant

let r: Result<i32, &str> = Ok(7);
let o: Option<i32> = r.ok();                            // Result -> Option (discards Err)
```

NEVER discard `Err` via `.ok()` in library code. Either propagate via `?` or transform via `.map_err(...)`.

---

## The `?` operator: desugaring and the From chain

`expr?` on a `Result<T, E>` desugars (roughly) to:

```rust
match expr {
    Ok(v)  => v,
    Err(e) => return Err(From::from(e)),
}
```

The enclosing function MUST return a `Result<_, E2>` where `E2: From<E>`. If the conversion is not available the compiler reports `E0277: ? couldn't convert the error: the trait From<E> for E2 is not implemented`.

```rust
use std::fs;
use std::io;
use std::num::ParseIntError;

#[derive(Debug)]
enum MyError {
    Io(io::Error),
    Parse(ParseIntError),
}

impl From<io::Error> for MyError {
    fn from(e: io::Error) -> Self { MyError::Io(e) }
}
impl From<ParseIntError> for MyError {
    fn from(e: ParseIntError) -> Self { MyError::Parse(e) }
}

fn parse_number_from_file(path: &str) -> Result<i32, MyError> {
    let s = fs::read_to_string(path)?;   // io::Error -> MyError via From
    let n: i32 = s.trim().parse()?;      // ParseIntError -> MyError via From
    Ok(n)
}
```

ALWAYS provide `From<InnerError>` for every error type that callers may propagate through `?`. NEVER write a `match expr { Ok(v)=>v, Err(e)=>return Err(MyError::X(e)) }` chain by hand: that is exactly what `?` does, and the manual form rots when error variants grow.

The same desugaring applies to `Option<T>` (`None` propagates), to `ControlFlow`, and to any type that implements the unstable `Try` trait. For stable code, mentally treat `?` as `Result-or-Option early return + From conversion`.

---

## The `Error` trait: `Display` + source chain

The `std::error::Error` trait is the contract for errors:

```rust
pub trait Error: Debug + Display {
    fn source(&self) -> Option<&(dyn Error + 'static)> { None }
    // (provided method; default returns None)
}
```

Any type that wants to participate in the wider error ecosystem (anyhow, eyre, `?` to `Box<dyn Error>`, error reporters) MUST implement:

1. `Debug` (derive it).
2. `Display` (write the user-facing message, NO trailing newline, NO leading "Error:" prefix).
3. `Error` (override `source()` to return the underlying cause when wrapping another error).

```rust
use std::fmt;

#[derive(Debug)]
struct ConfigError {
    path: String,
    source: std::io::Error,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to load config {}", self.path)
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
```

ALWAYS implement `source()` when your error *wraps* another error. Downstream tools (anyhow report, eyre's report, `tracing::error!`) walk the chain via `source()` to print the full cause.

NEVER cram the cause into the `Display` message (e.g. `format!("config failed: {}", inner)`). Report frameworks walk `source()` and would print the cause twice.

### `core::error::Error` (stabilized 1.81)

Since Rust 1.81 (2024-09-05) the `Error` trait is also available as `core::error::Error`, identical signature, usable in `no_std`. In `std` crates `std::error::Error` is a re-export of `core::error::Error`, so existing code is unaffected.

ALWAYS use `core::error::Error` if your crate is `no_std`. NEVER use `std::error::Error` in a `no_std` crate: the import will fail to resolve.

```rust
#![no_std]
extern crate alloc;
use core::error::Error;
use core::fmt;

#[derive(Debug)]
pub struct ProtocolError;

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("protocol violation")
    }
}

impl Error for ProtocolError {}
```

---

## Custom error type design: enum vs struct

| Shape | When |
|-------|------|
| `enum` with one variant per *distinct failure kind* | Multiple unrelated error sources (I/O, parse, validation). Each variant may carry a payload (the inner error, a path, a span). |
| `struct` with a `kind` field and a `source` | Single conceptual error with rich context (a database error code + the failing query + the cause). |
| `struct` (unit) | A single, payload-free failure mode. |

```rust
// Enum: one variant per kind
#[derive(Debug)]
pub enum LoadError {
    Io { path: String, source: std::io::Error },
    Parse { line: usize, source: serde_json::Error },
    SchemaViolation(String),
}

// Struct: single kind + context
#[derive(Debug)]
pub struct DbError {
    pub code: i32,
    pub query: String,
    pub source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}
```

Design rules:

- ALWAYS keep error types `Send + Sync + 'static` if they may cross thread or async boundaries. Box trait-object causes as `Box<dyn Error + Send + Sync + 'static>`.
- ALWAYS keep the error type *small* (one machine word per variant payload, boxing large payloads). `Result<T, E>` is returned by value; a fat `E` makes the `Ok` path slower.
- NEVER expose `String` as your only error type in a library API: callers cannot match on it. Strings are fine for application binaries.
- NEVER return `Box<dyn Error>` from a library function: callers lose the ability to match on specific variants. Use a concrete `enum` (typically via `thiserror`) and reserve `Box<dyn Error>` for *application* glue.

For the derive-macro route (`thiserror` for libraries, `anyhow` for binaries) see [[rust-errors-thiserror-anyhow]].

---

## Panic vs Result: the decision

| Failure category | Mechanism | Examples |
|------------------|-----------|----------|
| Expected, recoverable | `Result<T, E>` | File missing, parse failure, validation, network timeout. |
| Programmer bug, unrecoverable | `panic!` / `unwrap` / `expect` | Index out of bounds in code you control, broken invariant, `unreachable!()` branch. |

Rust Book Ch 9.3 ("To panic! or Not to panic!") rule: panic only when continuing would violate an invariant or risk corruption. Everything else is `Result`.

```rust
// Library API: return Result for expected failures.
pub fn load_user(id: u64) -> Result<User, LoadError> { /* ... */ }

// Programmer bug: panic.
fn assert_sorted(v: &[i32]) {
    debug_assert!(v.windows(2).all(|w| w[0] <= w[1]), "input must be sorted");
}

// Application top-level: unwrap with a message documenting the invariant.
fn main() {
    let cfg = load_config().expect("config.toml is required at startup");
}
```

NEVER use `.unwrap()` in library code. It turns an expected error into an unrecoverable panic and the caller cannot intercept it. Use `?` and let the caller decide.

NEVER use `.expect("...")` to log a recoverable error: the message goes to stderr only on panic, and the application aborts.

ALWAYS use `expect(msg)` over `unwrap()` when you do panic intentionally: the message documents *which invariant* you are asserting, making the panic transcript actionable.

ALWAYS use `assert!`, `debug_assert!`, or `unreachable!()` for invariant checks. Match arms that should be impossible: `unreachable!("variant X cannot occur after step Y")`.

---

## Result combinators: avoid the match ladder

`Result` has a rich combinator API. Prefer combinators over multi-line `match` blocks when the logic is one step:

```rust
let r: Result<i32, String> = Ok(7);

let doubled:  Result<i32, String> = r.map(|x| x * 2);                  // transform Ok
let labeled:  Result<i32, String> = r.map_err(|e| format!("bad: {e}"));// transform Err
let chained:  Result<i32, String> = r.and_then(|x| Ok(x + 1));         // chain Result-returning fn
let recover:  Result<i32, String> = r.or_else(|_e| Ok(0));             // recover from Err

let val:  i32 = r.unwrap_or(0);                                        // default on Err
let val2: i32 = r.unwrap_or_else(|_e| 0);                              // lazy default
let val3: i32 = r.unwrap_or_default();                                 // T: Default
```

For `Option<T>` <-> `Result<T, E>`:

```rust
let o: Option<i32> = Some(7);
let r: Result<i32, &str> = o.ok_or("missing");
let r2: Result<i32, &str> = o.ok_or_else(|| "missing");
```

ALWAYS use `map_err` to convert an error into your function's error type before `?`. Either `From` (automatic) or `map_err` (explicit) MUST bridge the inner error type to the outer.

NEVER write `.map(|x| Ok(x))` followed by a flatten: that is `and_then` for `Result` and is built-in.

NEVER write `if let Err(e) = result { return Err(e.into()); } else { result.unwrap() }`: that is `?`, written badly.

A full method table lives in `references/methods.md`.

---

## Source chain: walking the cause

When errors wrap other errors, callers walk the `source()` chain to print the full context. The pattern:

```rust
fn print_chain(e: &dyn std::error::Error) {
    eprintln!("error: {e}");
    let mut cur = e.source();
    while let Some(src) = cur {
        eprintln!("caused by: {src}");
        cur = src.source();
    }
}
```

Inside the error type, return the wrapped error from `source()`:

```rust
impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)   // <- the wrapped error
    }
}
```

In application code (binary crate) use `anyhow::Context::context("loading config")` to push a layer on the chain. See [[rust-errors-thiserror-anyhow]].

ALWAYS provide a `source()` impl when your error variant carries an inner error. NEVER mash the inner error into the `Display` text: report tools will then duplicate the cause when they walk the chain.

---

## Decision tree: error-handling strategy

```
Operation can fail?
|
+-- No (only programmer bugs): use assert!/debug_assert!/unreachable!.
|
+-- Yes:
    |
    +-- Library crate?
    |   |
    |   +-- Return Result<T, E> with a concrete `enum` error type (often via thiserror).
    |   +-- Implement Display + Error (+ source() if wrapping inner errors).
    |   +-- Implement From<InnerError> for every error you propagate via ?.
    |   +-- NEVER use anyhow::Error in public API. NEVER unwrap/expect.
    |
    +-- Binary / application crate?
        |
        +-- Use anyhow::Result<T> at boundaries; .context("...") at each layer.
        +-- main() -> anyhow::Result<()> prints the chain on Err.
        +-- At the very top: expect("invariant X") is acceptable for startup-only requirements.
```

See [[rust-errors-thiserror-anyhow]] for the derive-macro details.

---

## Quick reference table

| Item | One-line rule |
|------|---------------|
| `Result<T, E>` | Success or failure with reason. Default for fallible APIs. |
| `Option<T>` | Presence or absence, no reason. Wrong choice if caller needs to know "why". |
| `?` operator | Early-return on `Err`, applies `From::from`. Function must return `Result<_, E2: From<inner>>`. |
| `From` impl | The conversion `?` uses. One impl per inner error you propagate. |
| `Error` trait | `Debug + Display + source()`. Required for ecosystem participation. |
| `core::error::Error` | Same trait, since 1.81. Use this in `no_std`. |
| `Display` | One-line user-facing message. No newline. No leading "Error:". |
| `source()` | Returns the underlying cause for chain-walking. |
| `panic!` / `unwrap` / `expect` | Programmer bugs only. NEVER in library code. |
| `map_err` | Transform an `Err` value (typically into your function's error type). |
| `and_then` | Chain a `Result`-returning function. |
| `or_else` | Recover from `Err` by computing a new `Result`. |
| `ok_or` / `ok_or_else` | `Option<T>` -> `Result<T, E>`. |
| `unwrap_or` / `unwrap_or_default` / `unwrap_or_else` | Extract `T` with a fallback. Safe in any code. |
| `Box<dyn Error + Send + Sync + 'static>` | Trait-object error. Fine inside an error variant, NEVER as public API return. |

---

## Avoid these mistakes

(Full list with WHYs in `references/anti-patterns.md`.)

- `.unwrap()` or `.expect()` in library code: turns a recoverable `Err` into a process-killing panic. Use `?` and let the caller decide.
- Defining an error type without `Display` + `Error` impls: it cannot compose with `?`, anyhow, eyre, or `Box<dyn Error>`. Always implement both.
- Forgetting `From<InnerError>` for `MyError`: `?` fails to compile with `E0277` and you fall back to `match` ladders. Always add `From` per inner error.
- Returning `Box<dyn Error>` from a library function: callers lose the ability to match on variants. Return a concrete `enum`; reserve `Box<dyn Error>` for application glue.
- `Result<(), ()>`: lossy, the `Err` carries no information. Use a real error type, even a unit struct with a meaningful name.
- `panic!` for expected failure modes (parse error, missing file, network down): callers cannot recover. Return `Result`.
- Stuffing the inner error into the `Display` message AND returning it from `source()`: report tools print the cause twice. Pick one (always `source()`).
- Using `std::error::Error` in a `no_std` crate (since 1.81): the import does not resolve. Use `core::error::Error`.
- `.ok()` to discard an `Err` in library code: silently swallows failures. Either propagate via `?` or convert via `map_err`.
- Mixing `anyhow::Error` into a library's public API: downstream cannot match on variants, your error type is forever opaque. anyhow belongs in binaries.

---

## Reference links

[std-result]: https://doc.rust-lang.org/std/result/
[std-error]: https://doc.rust-lang.org/std/error/trait.Error.html
[core-error]: https://doc.rust-lang.org/core/error/trait.Error.html
[book-ch9]: https://doc.rust-lang.org/book/ch09-00-error-handling.html
[book-ch9-3]: https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html
[blog-1-81]: https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/
[rfc-question-mark]: https://rust-lang.github.io/rfcs/0243-trait-based-exception-handling.html
[std-option]: https://doc.rust-lang.org/std/option/

- [`std::result`][std-result]: full `Result` API.
- [`std::option`][std-option]: full `Option` API and the conversions to `Result`.
- [`std::error::Error`][std-error]: the trait, `source()`, downcasting rules.
- [`core::error::Error`][core-error]: identical trait, no_std-friendly (stable since 1.81).
- [Rust Book Ch 9][book-ch9]: `Result`, `?`, panic vs Result.
- [Rust Book Ch 9.3][book-ch9-3]: "To panic! or Not to panic!" decision criteria.
- [Rust 1.81.0 release][blog-1-81]: announcement of `core::error::Error` stabilization.
- [RFC 0243][rfc-question-mark]: the `?` operator design.

For deeper drill-downs see:

- `references/methods.md`: complete signatures for `Result`, `Option`, `Error`, and the `From`/`?` desugaring.
- `references/examples.md`: working code for custom enum errors, source chain, `no_std` errors, combinator chains.
- `references/anti-patterns.md`: 10+ documented anti-patterns with WHY and FIX.
