# rust-syntax-lifetimes: anti-patterns

Every entry: the rejected code, the compiler or Clippy message, the deterministic fix.

## AP1: Over-annotating lifetimes that elision already infers

### Rejected (lint, not error)

```rust
fn first<'a>(s: &'a str) -> &'a str {
    s.split_whitespace().next().unwrap_or("")
}
```

### Lint

```
warning: the following explicit lifetimes could be elided: 'a
  --> src/lib.rs:1:9
  = note: `#[warn(clippy::needless_lifetimes)]` on by default
```

### Fix

ALWAYS rely on elision when it applies:

```rust
fn first(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}
```

## AP2: Using `T: 'static` to silence the compiler

### Rejected (technically compiles, design smell)

```rust
struct Registry<T: 'static> { items: Vec<T> }
fn register<T: 'static>(r: &mut Registry<T>, t: T) { r.items.push(t); }
```

The `T: 'static` bound forces every caller to provide a value with no shorter borrows, even when the registry never escapes the function.

### Fix

Plumb a real lifetime parameter when the data does NOT need to outlive the program:

```rust
struct Registry<'a, T> { items: Vec<&'a T> }
fn register<'a, T>(r: &mut Registry<'a, T>, t: &'a T) { r.items.push(t); }
```

Use `T: 'static` ONLY when the value will actually be stored beyond every caller frame (a `static`, a `thread::spawn` closure, a long-lived global registry).

## AP3: Confusing `T: 'static` with `&'static T`

### Wrong mental model

"`String` is `'static` so it must be a `&'static str`."

### Reality

- `String: 'static` is true because `String` contains no borrows that could be shorter than `'static`.
- `&'static str` is a string slice whose REFERENCED DATA lives the entire program.

### Demonstration

```rust
fn requires_static_bound<T: 'static>(_: T) {}
fn requires_static_ref(_: &'static str) {}

let owned: String = String::from("hi");
requires_static_bound(owned);          // OK: String: 'static
// requires_static_ref(&owned);        // ERROR: &owned is &'_ str, not &'static str
let lit: &'static str = "hi";
requires_static_ref(lit);              // OK
requires_static_bound(lit);            // OK: &'static str also satisfies T: 'static
```

ALWAYS distinguish a TYPE BOUND from a REFERENCE TYPE before writing `'static`.

## AP4: Assuming `&mut T` is covariant in `T`

### Rejected

```rust
fn assign<'a>(target: &mut &'a str, src: &'a str) {
    *target = src;
}

fn main() {
    let mut s: &'static str = "hi";
    let local = String::from("local");
    assign(&mut s, &local);
    println!("{s}"); // would dangle if compiled
}
```

### Compiler

```
error[E0597]: `local` does not live long enough
```

### Why

`&'a mut T` is INVARIANT in `T`. The compiler refuses to widen `&'a mut &'static str` to `&'a mut &'short str`, because that would let you write a short borrow into a slot expected to hold a `&'static str`.

### Fix

NEVER pass references whose inner lifetimes do not exactly match through `&mut`. Either restructure the data so only owned values pass through `&mut`, or ensure the inner reference has a single common lifetime.

## AP5: Pre-2024 RPIT signature that fails in edition 2024

### Rejected (only fails for some users, depending on overcapture lint)

```rust
fn legacy<'a, T: Clone>(v: &'a T) -> impl std::fmt::Debug {
    v.clone()
}
```

In edition 2024 this captures BOTH `'a` AND `T` by default. Callers that previously relied on `'a` NOT being captured see new "does not live long enough" failures.

### Compiler

```
warning: `impl Trait` will capture more lifetimes than possibly intended in edition 2024
  = note: specifically, this lifetime is in scope but not mentioned in the type's bounds: `'a`
  = help: to keep the current behavior, add explicit captures with `+ use<T>`
```

### Fix

ALWAYS add `+ use<...>` to opt OUT of the new default when older API stability is required:

```rust
fn legacy<'a, T: Clone>(v: &'a T) -> impl std::fmt::Debug + use<T> {
    v.clone()
}
```

Or run `cargo fix --edition` to apply the `impl_trait_overcaptures` lint automatically.

## AP6: Manually scoping a borrow that NLL would already end

### Rejected (works, but wasteful)

```rust
fn main() {
    let mut v = vec![1, 2, 3];
    {
        let first = &v[0];
        println!("{first}");
    } // artificial scope
    v.push(4);
}
```

### Compiler

No error; NLL would have ended the borrow at the last use of `first` without the extra braces.

### Fix

NEVER introduce artificial `{ }` to end a borrow. Trust NLL:

```rust
fn main() {
    let mut v = vec![1, 2, 3];
    let first = &v[0];
    println!("{first}");
    v.push(4);
}
```

## AP7: Writing `for<'a>` when a single generic `'a` would do

### Rejected (works, but obscure)

```rust
fn run<F>(f: F) where F: for<'a> Fn(&'a i32) -> i32 {
    let x = 1;
    f(&x);
}
```

### When HRTB is unnecessary

If the closure is only ever called with one specific lifetime, a plain generic suffices:

```rust
fn run<'a, F>(x: &'a i32, f: F) -> i32 where F: Fn(&'a i32) -> i32 {
    f(x)
}
```

ALWAYS reach for HRTB ONLY when the function or trait must accept closures usable at MULTIPLE lifetimes (typical for stored or generic-callback APIs).

## AP8: Adding `+ 'a` to RPIT instead of using `use<>`

### Rejected on edition 2024

```rust
fn iter<'a, T>(s: &'a [T]) -> impl Iterator<Item = &'a T> + 'a { s.iter() }
```

The trailing `+ 'a` is now redundant: edition 2024 already captures `'a`. Worse, mixing the bound with `+ use<>` is a hard error.

### Fix

ALWAYS drop the trailing `+ 'a` in edition 2024; rely on the default capture, and use `+ use<...>` ONLY to RESTRICT capture:

```rust
fn iter<'a, T>(s: &'a [T]) -> impl Iterator<Item = &'a T> { s.iter() }
```

## AP9: Using `'static` to fix `thread::spawn` "does not live long enough"

### Rejected

```rust
use std::thread;

fn spawn_with(s: &str) {
    thread::spawn(|| println!("{s}")); // ERROR: `s` may not live long enough
}
```

### Wrong fix

Bumping `s: &'static str` is wrong if the caller cannot guarantee `'static`.

### Right fix

ALWAYS move owned data into the closure with `move`, NEVER add `'static` to the parameter:

```rust
use std::thread;

fn spawn_with(s: String) {
    thread::spawn(move || println!("{s}"));
}
```

The thread now owns the `String`; no lifetime concerns remain.

## AP10: Forgetting `where Self: 'a` on a GAT

### Rejected

```rust
trait Container {
    type Item<'a>;
    fn get(&self) -> Self::Item<'_>;
}
```

### Compiler

```
error: missing required bound on `Item`
  = help: add `where Self: 'a`
```

### Fix

ALWAYS add the `where Self: 'a` bound on generic associated types that borrow from `self`:

```rust
trait Container {
    type Item<'a> where Self: 'a;
    fn get(&self) -> Self::Item<'_>;
}
```

See `[[rust-syntax-gats]]` for the full GAT pattern.

## AP11: Using lifetime parameters in `let` bindings

### Rejected

```rust
fn main() {
    let s: &'a str = "hi"; // ERROR: undeclared lifetime `'a`
}
```

### Compiler

```
error[E0261]: use of undeclared lifetime name `'a`
```

### Fix

ALWAYS let the compiler infer lifetimes for local bindings. NEVER attempt to annotate a lifetime on a `let`:

```rust
fn main() {
    let s: &str = "hi"; // compiler infers 'static
}
```

If you genuinely need `'static`, write `&'static str`; otherwise omit the lifetime.

## AP12: Returning `&str` from a function that allocates

### Rejected

```rust
fn build_name() -> &str {
    let s = String::from("hello");
    &s
}
```

### Compiler

```
error[E0106]: missing lifetime specifier
error[E0515]: cannot return reference to local variable `s`
```

### Fix

NEVER return a reference to a local. Either return an owned `String`, or take a buffer parameter:

```rust
fn build_name() -> String {
    String::from("hello")
}
```

Cross-reference: `[[rust-errors-lifetimes]]` for the full catalogue of lifetime-related compiler errors.
