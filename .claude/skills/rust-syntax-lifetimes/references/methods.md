# rust-syntax-lifetimes: methods reference

Complete grammar of lifetime annotations on every Rust item kind. Verified against the Rust Reference (edition 2024).

## 1. Function items

### Generic lifetime parameter

```rust
fn foo<'a>(x: &'a str) -> &'a str { x }
```

`'a` is introduced inside the angle brackets and may appear on any input or output reference. Multiple lifetime parameters are comma-separated.

### Multiple lifetimes

```rust
fn pick_first<'a, 'b>(x: &'a str, _y: &'b str) -> &'a str { x }
```

### Lifetime bound on a lifetime (`'a: 'b`)

Reads "`'a` outlives `'b`". Introduced inside the parameter list or in a `where` clause:

```rust
fn longer<'a, 'b>(x: &'a str, y: &'b str) -> &'a str
where 'b: 'a {
    if x.len() > y.len() { x } else { y }
}
```

### Lifetime bound on a type (`T: 'a`)

Reads "every reference inside `T` outlives `'a`":

```rust
fn ref_to<'a, T: 'a>(t: &'a T) -> &'a T { t }
```

### `'static` bound

`T: 'static` is satisfied by owned types and by `&'static U` references:

```rust
fn store<T: 'static>(_t: T) { /* ... */ }
```

## 2. Method items

### Receiver lifetime via elision rule 3

```rust
struct Wrap<'a>(&'a str);

impl<'a> Wrap<'a> {
    fn get(&self) -> &str { self.0 }   // output lifetime = self's
}
```

### Explicit method lifetime that differs from `Self`'s

```rust
impl<'a> Wrap<'a> {
    fn merge<'b>(&self, other: &'b str) -> &str
    where 'a: 'b {
        if self.0.len() > other.len() { self.0 } else { other }
    }
}
```

## 3. Struct, enum, union items

NO elision applies. Every reference field requires an explicit lifetime parameter on the type.

```rust
struct Pair<'a, 'b> {
    a: &'a str,
    b: &'b str,
}

enum Either<'a, T> {
    Borrowed(&'a T),
    Owned(T),
}

union Slot<'a> {
    r: &'a i32,
    n: u64,
}
```

## 4. Trait items

### Lifetime parameter on a trait

```rust
trait Parser<'input> {
    fn parse(&self, src: &'input str) -> &'input str;
}
```

### Lifetime bound on associated type

```rust
trait Container {
    type Item<'a> where Self: 'a;
}
```

This is a Generic Associated Type (GAT); see `[[rust-syntax-gats]]`.

### Trait object lifetime

`dyn Trait + 'a` annotates how long the trait object's referenced data is valid. Default lifetimes for `Box<dyn Trait>` is `'static`; for `&dyn Trait` it is the lifetime of the reference.

```rust
fn make() -> Box<dyn std::fmt::Debug + 'static> { Box::new(42) }
fn take(_t: &(dyn std::fmt::Debug + '_)) {}
```

## 5. Impl items

The lifetime parameter is introduced on the `impl` keyword and may appear on the type:

```rust
impl<'a> Pair<'a, 'a> {
    fn same_lifetime(&self) -> &str { self.a }
}
```

## 6. `where` clauses

`where` clauses are equivalent to inline bounds but read better for many constraints:

```rust
fn complex<'a, 'b, T, U>(t: &'a T, u: &'b U) -> &'a T
where
    'b: 'a,
    T: 'a,
    U: std::fmt::Debug + 'b,
{
    t
}
```

## 7. Higher-Ranked Trait Bounds

Quantify over all lifetimes:

```rust
fn run<F>(f: F) where F: for<'a> Fn(&'a str) -> &'a str { /* ... */ }
```

Also valid in trait bounds and `dyn` types:

```rust
let _: Box<dyn for<'a> Fn(&'a i32)> = Box::new(|_| ());
```

## 8. RPIT (return-position `impl Trait`) capture

Edition 2024 default: capture all in-scope generics.

```rust
fn iter<'a, T>(slice: &'a [T]) -> impl Iterator<Item = &'a T> {
    slice.iter()
}
```

Explicit `+ use<...>` overrides default:

```rust
fn iter_no_t<'a, T>(slice: &'a [T]) -> impl Iterator<Item = &'a T> + use<'a, T> {
    slice.iter()
}

fn nothing_captured() -> impl std::fmt::Debug + use<> { 42 }
```

## 9. Lifetime keywords

| Keyword | Meaning |
|---------|---------|
| `'static` | the entire program duration |
| `'_` | anonymous lifetime (let elision decide or place-holder in impls) |
| `'a` (any lowercase) | named lifetime parameter |

`'_` is permitted in `impl<'_>` and type position to invoke elision explicitly:

```rust
impl Wrap<'_> {
    fn anon(&self) -> &str { self.0 }
}

fn shorter(s: &str) -> &'_ str { s }
```

## 10. Variance opt-in via `PhantomData`

To declare variance manually on an unsafe abstraction, embed an appropriate `PhantomData`:

```rust
use std::marker::PhantomData;
struct Covariant<'a, T>(*const T, PhantomData<&'a T>);
struct Invariant<'a, T>(*mut T, PhantomData<&'a mut T>);
struct Contravariant<'a, T>(*const T, PhantomData<fn(&'a T)>);
```

See `https://doc.rust-lang.org/nomicon/subtyping.html` for the full table.

## 11. Lifetime parameters MUST come first

In any generic parameter list, lifetime parameters precede type parameters which precede const parameters:

```rust
fn ordering<'a, 'b, T, const N: usize>(_: &'a [T; N]) {}
```
