# methods.md: error-handling APIs

Complete signatures, sourced from `std::result`, `std::option`, `std::error`, `core::error`, and the Rust Reference.

---

## `Result<T, E>` core methods

```rust
// Construction
Result::Ok(T)            // success carrying T
Result::Err(E)           // failure carrying E

// Predicates
pub const fn is_ok(&self) -> bool;
pub const fn is_err(&self) -> bool;
pub fn is_ok_and(self, f: impl FnOnce(T) -> bool) -> bool;   // 1.70+
pub fn is_err_and(self, f: impl FnOnce(E) -> bool) -> bool;  // 1.70+

// Inspect without consuming
pub fn as_ref(&self) -> Result<&T, &E>;
pub fn as_mut(&mut self) -> Result<&mut T, &mut E>;
pub fn iter(&self) -> Iter<'_, T>;
pub fn iter_mut(&mut self) -> IterMut<'_, T>;

// Extract or panic
pub fn unwrap(self) -> T where E: Debug;
pub fn expect(self, msg: &str) -> T where E: Debug;
pub fn unwrap_err(self) -> E where T: Debug;
pub fn expect_err(self, msg: &str) -> E where T: Debug;

// Extract with fallback (never panic)
pub fn unwrap_or(self, default: T) -> T;
pub fn unwrap_or_else(self, op: impl FnOnce(E) -> T) -> T;
pub fn unwrap_or_default(self) -> T where T: Default;

// Transform
pub fn map<U, F: FnOnce(T) -> U>(self, op: F) -> Result<U, E>;
pub fn map_err<F, O: FnOnce(E) -> F>(self, op: O) -> Result<T, F>;
pub fn map_or<U, F: FnOnce(T) -> U>(self, default: U, f: F) -> U;
pub fn map_or_else<U, D: FnOnce(E) -> U, F: FnOnce(T) -> U>(self, default: D, f: F) -> U;

// Chain
pub fn and<U>(self, res: Result<U, E>) -> Result<U, E>;
pub fn and_then<U, F: FnOnce(T) -> Result<U, E>>(self, op: F) -> Result<U, E>;
pub fn or<F>(self, res: Result<T, F>) -> Result<T, F>;
pub fn or_else<F, O: FnOnce(E) -> Result<T, F>>(self, op: O) -> Result<T, F>;

// Conversion to Option
pub fn ok(self) -> Option<T>;        // discards Err
pub fn err(self) -> Option<E>;       // discards Ok

// Inspection (side-effect, returns self)         // 1.76+
pub fn inspect(self, f: impl FnOnce(&T)) -> Self;
pub fn inspect_err(self, f: impl FnOnce(&E)) -> Self;
```

`Result<T, E>: ?` works iff the enclosing function returns `Result<_, E2>` and `E2: From<E>`. The trait route is `core::ops::Try` (unstable); the stable observation is the `From` requirement.

---

## `Option<T>` <-> `Result<T, E>` bridges

```rust
// Option -> Result
pub fn ok_or<E>(self, err: E) -> Result<T, E>;
pub fn ok_or_else<E, F: FnOnce() -> E>(self, err: F) -> Result<T, E>;

// Result -> Option
pub fn ok(self) -> Option<T>;
pub fn err(self) -> Option<E>;

// Option chaining (parallel to Result)
pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Option<U>;
pub fn and_then<U, F: FnOnce(T) -> Option<U>>(self, f: F) -> Option<U>;
pub fn or(self, optb: Option<T>) -> Option<T>;
pub fn or_else<F: FnOnce() -> Option<T>>(self, f: F) -> Option<T>;
pub fn unwrap_or(self, default: T) -> T;
pub fn unwrap_or_else<F: FnOnce() -> T>(self, f: F) -> T;
pub fn unwrap_or_default(self) -> T where T: Default;
```

---

## The `Error` trait (`std::error::Error`, `core::error::Error`)

Identical signature since Rust 1.81 (2024-09-05). `std::error::Error` is a re-export of `core::error::Error`.

```rust
pub trait Error: Debug + Display {
    // PROVIDED method. Default returns None.
    fn source(&self) -> Option<&(dyn Error + 'static)> { None }

    // DEPRECATED in favor of source(). Do not override.
    #[allow(deprecated)]
    fn description(&self) -> &str { "description() is deprecated; use Display" }
    #[allow(deprecated)]
    fn cause(&self) -> Option<&dyn Error> { self.source() }

    // UNSTABLE (provider API): for typed context retrieval.
    // fn provide<'a>(&'a self, request: &mut Request<'a>) { }
}
```

Required impls when defining a new error type:

| Trait | How |
|-------|-----|
| `Debug` | `#[derive(Debug)]`. Always derive. |
| `Display` | Manual impl: write one user-facing line, no newline. |
| `Error` | Manual impl (or via thiserror). Override `source()` iff you wrap an inner error. |

Marker bounds you typically also want:

| Bound | Why |
|-------|-----|
| `Send` | Cross-thread propagation, panic payloads, async tasks. |
| `Sync` | Shared-reference access across threads. |
| `'static` | `Box<dyn Error>` requires it; downcasting requires it. |

The conventional public bound for trait-object errors is `Box<dyn Error + Send + Sync + 'static>` (the bound `anyhow::Error` uses internally).

### Downcasting (when the error is a trait object)

```rust
pub fn downcast<E: Error + 'static>(self: Box<Self>) -> Result<Box<E>, Box<Self>>;
pub fn downcast_ref<E: Error + 'static>(&self) -> Option<&E>;
pub fn downcast_mut<E: Error + 'static>(&mut self) -> Option<&mut E>;
```

Only works on the **concrete** type; downcasting to a trait does not work. Walk `source()` and downcast each link.

---

## The `From` trait (the `?` conversion)

```rust
pub trait From<T>: Sized {
    fn from(value: T) -> Self;
}
```

The reflexive `impl<T> From<T> for T` is provided automatically (`From<MyError> for MyError` always exists). For every *other* inner error, you write:

```rust
impl From<std::io::Error> for MyError {
    fn from(e: std::io::Error) -> Self { MyError::Io(e) }
}
```

`Into` is the dual; you never implement it directly (the blanket `impl<T, U: From<T>> Into<U> for T` provides it).

The `?` operator calls `From::from(inner_err)` on the `Err` value before returning it. If `From` is missing, the compiler emits `E0277` ("the trait bound `From<E> for E2` is not satisfied").

---

## The `?` operator: precise desugaring

For `Result`:

```rust
// expr?
match expr {
    Ok(v)  => v,
    Err(e) => return Err(From::from(e)),
}
```

For `Option`:

```rust
// expr?
match expr {
    Some(v) => v,
    None    => return None,
}
```

Constraints (stable Rust):

| Container | Function return must be | Conversion |
|-----------|-------------------------|------------|
| `Result<T, E>` | `Result<_, E2>` with `E2: From<E>` | `From::from(e)` |
| `Option<T>` | `Option<_>` | none (just `None`) |

Mixing: you cannot `?` an `Option<T>` in a function that returns `Result<T, E>`. Convert first via `.ok_or(...)`.

```rust
fn parse(s: Option<&str>) -> Result<i32, String> {
    let s = s.ok_or("missing")?;        // Option -> Result, then ?
    let n: i32 = s.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
    Ok(n)
}
```

The `?` operator also threads `core::ops::ControlFlow` and any future `Try`-implementing type. For day-to-day code think `Result`-or-`Option`.

---

## Combinator cheat-sheet

| Goal | Operation |
|------|-----------|
| Transform `Ok` value | `r.map(f)` |
| Transform `Err` value | `r.map_err(f)` |
| Chain into another `Result` | `r.and_then(f)` |
| Recover from `Err` | `r.or_else(f)` or `.unwrap_or_else(f)` |
| Default on `Err` | `r.unwrap_or(default)` / `r.unwrap_or_default()` |
| Drop `Err` | `r.ok()` (use sparingly) |
| Drop `Ok` | `r.err()` |
| Side-effect on `Ok` | `r.inspect(|x| ...)`  (1.76+) |
| Side-effect on `Err` | `r.inspect_err(|e| ...)` (1.76+) |
| Option -> Result | `o.ok_or(err)` / `o.ok_or_else(\|\| err)` |
| Result -> Option (Ok) | `r.ok()` |
| Result -> Option (Err) | `r.err()` |

Use the combinator that names your intent; readers should not have to expand it back to a `match`.

---

## `Box<dyn Error>` and friends

```rust
// Inside an error variant: fine, captures a heterogeneous cause.
struct Wrap { source: Box<dyn std::error::Error + Send + Sync + 'static> }

// As a function return type in a binary main(): fine, simple top-level glue.
fn main() -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

// As a library public API return type: do NOT. Callers lose type info.
// Use a concrete enum + thiserror instead. See [[rust-errors-thiserror-anyhow]].
```

The `Send + Sync + 'static` bound is the standard contract: it allows the error to cross thread boundaries, be stored in `tokio::task::JoinError`, and be downcast.

---

## Sources

- [`std::result`](https://doc.rust-lang.org/std/result/)
- [`std::option`](https://doc.rust-lang.org/std/option/)
- [`std::error::Error`](https://doc.rust-lang.org/std/error/trait.Error.html)
- [`core::error::Error`](https://doc.rust-lang.org/core/error/trait.Error.html)
- [Rust 1.81.0 release notes](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/)
- [Rust Reference: the `?` operator](https://doc.rust-lang.org/reference/expressions/operator-expr.html#the-question-mark-operator)
