# Worked examples: traits, impls, derives, marker traits

Every example is a self-contained file you can drop into `src/lib.rs` (or `src/main.rs` for the `fn main` ones) of a Cargo crate on Rust 1.85+ with edition 2024.

---

## Example 1: Required vs provided methods

```rust
pub trait Greeter {
    // Required: every implementor must provide this.
    fn name(&self) -> &str;

    // Provided default: implementors may override.
    fn greet(&self) -> String {
        format!("Hello, {}!", self.name())
    }

    // Provided default that calls another provided method.
    fn loud_greet(&self) -> String {
        self.greet().to_uppercase()
    }
}

pub struct World;
impl Greeter for World {
    fn name(&self) -> &str { "world" }
    // greet() and loud_greet() use defaults.
}

pub struct Shout;
impl Greeter for Shout {
    fn name(&self) -> &str { "shout" }
    fn greet(&self) -> String { "HI THERE".into() }  // override
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn world_uses_defaults() {
        assert_eq!(World.greet(), "Hello, world!");
        assert_eq!(World.loud_greet(), "HELLO, WORLD!");
    }
    #[test]
    fn shout_overrides() {
        assert_eq!(Shout.greet(), "HI THERE");
        assert_eq!(Shout.loud_greet(), "HI THERE");
    }
}
```

---

## Example 2: Supertraits

```rust
use std::fmt::Display;

pub trait Shape {
    fn area(&self) -> f64;
}

// `Display` is a supertrait of `Pretty`. Any `Pretty` type must implement
// `Display` AND `Shape`. Inside default methods we can use both.
pub trait Pretty: Shape + Display {
    fn pretty(&self) -> String {
        format!("{self} with area {:.2}", self.area())
    }
}

pub struct Square { side: f64 }
impl Shape for Square { fn area(&self) -> f64 { self.side * self.side } }
impl Display for Square {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Square({} x {})", self.side, self.side)
    }
}
impl Pretty for Square {}   // gets `pretty()` for free
```

---

## Example 3: Blanket impl

```rust
pub trait Stringify {
    fn stringify(&self) -> String;
}

// Blanket: every Display type is automatically Stringify.
impl<T: std::fmt::Display> Stringify for T {
    fn stringify(&self) -> String { format!("{self}") }
}

fn demo() {
    let s = 42_i32.stringify();       // works for any Display type
    assert_eq!(s, "42");
}
```

Adding a non-blanket impl like `impl Stringify for u32 {}` on the side would fail with E0119 because `u32: Display` is already covered by the blanket.

---

## Example 4: Sealed trait (private supertrait pattern)

```rust
// src/lib.rs
pub trait Operation: private::Sealed {
    fn run(&self) -> u32;
}

mod private {
    pub trait Sealed {}
}

pub struct Add;
impl private::Sealed for Add {}
impl Operation for Add { fn run(&self) -> u32 { 3 } }

pub struct Mul;
impl private::Sealed for Mul {}
impl Operation for Mul { fn run(&self) -> u32 { 6 } }

// Downstream consumers can call Operation methods but cannot implement it.
// If a user crate writes `impl Operation for MyType {}`, the compiler will
// reject it with E0445 / E0446 because `private::Sealed` is not nameable
// outside this crate.
```

---

## Example 5: Newtype wrapper for foreign trait + foreign type

```rust
use std::fmt;

// We cannot `impl fmt::Display for Vec<u8>`: both are foreign (E0117).
// Wrap Vec<u8> in a local newtype, implement Display on the newtype.
pub struct Hex(pub Vec<u8>);

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 { write!(f, "{b:02x}")?; }
        Ok(())
    }
}

fn demo() {
    let h = Hex(vec![0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(format!("{h}"), "deadbeef");
}
```

---

## Example 6: Marker trait + auto trait holes via PhantomData

```rust
use std::marker::PhantomData;

// This struct is `Send` and `Sync` automatically (auto traits).
pub struct Plain { data: Vec<u8> }

// Opt out of Send/Sync by including a !Send !Sync witness:
pub struct ThreadLocal {
    data: Vec<u8>,
    _not_send_not_sync: PhantomData<*mut ()>,  // raw ptr is !Send !Sync
}

fn _check_send<T: Send>() {}
// _check_send::<Plain>();         // OK: Plain is Send
// _check_send::<ThreadLocal>();   // E0277: ThreadLocal is not Send
```

---

## Example 7: Inherent impl vs trait impl (and shadowing)

```rust
struct Counter { n: u32 }

// Inherent impl
impl Counter {
    fn new() -> Self { Self { n: 0 } }
    fn next(&mut self) -> u32 {           // inherent method, shadows Iterator::next
        self.n += 1;
        self.n
    }
}

impl Iterator for Counter {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {   // trait method, shadowed in dot syntax
        let v = self.n + 1;
        self.n = v;
        Some(v)
    }
}

fn demo() {
    let mut c = Counter::new();
    let inherent_result: u32 = c.next();             // inherent wins
    let trait_result: Option<u32> = Iterator::next(&mut c);   // fully qualified
    assert_eq!(inherent_result, 1);
    assert_eq!(trait_result, Some(2));
}
```

---

## Example 8: Derive Copy, Clone, Debug, PartialEq, Eq, Hash, Default

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

fn demo() {
    let a = Point { x: 1, y: 2 };
    let b = a;                         // Copy (no move)
    assert_eq!(a, b);                  // PartialEq
    assert_eq!(format!("{a:?}"), "Point { x: 1, y: 2 }");  // Debug

    let mut set = std::collections::HashSet::new();
    set.insert(a);                     // Hash + Eq
    assert!(set.contains(&b));

    let z: Point = Default::default(); // Default
    assert_eq!(z, Point { x: 0, y: 0 });
}
```

Adding a `String` field would force the removal of `Copy` (String is not `Copy`).

Adding `impl Drop for Point { ... }` would force the removal of `Copy` (E0184).

---

## Example 9: Generic blanket impl with multiple bounds

```rust
use std::hash::Hash;
use std::collections::HashMap;

pub trait Counter<K> {
    fn count(self) -> HashMap<K, usize>;
}

// Blanket impl: every iterator yielding hashable items can be counted.
impl<I, K> Counter<K> for I
where
    I: IntoIterator<Item = K>,
    K: Eq + Hash,
{
    fn count(self) -> HashMap<K, usize> {
        let mut m = HashMap::new();
        for k in self { *m.entry(k).or_insert(0) += 1; }
        m
    }
}

fn demo() {
    let m = vec!["a", "b", "a", "c", "b", "a"].count();
    assert_eq!(m["a"], 3);
    assert_eq!(m["b"], 2);
    assert_eq!(m["c"], 1);
}
```

---

## Example 10: Adding a method to a published trait safely

```rust
// Bad: a hard breaking change because all existing impls now lack `metadata`.
// pub trait Source { fn data(&self) -> &str; fn metadata(&self) -> &str; }

// Good: provide a default so existing impls continue to compile.
pub trait Source {
    fn data(&self) -> &str;

    // New method added in version 1.1; existing impls inherit this default.
    fn metadata(&self) -> &str { "" }
}

pub struct FileSource { contents: String }
impl Source for FileSource {
    fn data(&self) -> &str { &self.contents }
    // metadata() not overridden; uses default.
}
```

ALWAYS add new trait methods with a default body to keep the addition non-breaking. Combine with the sealed-trait pattern if you ALSO want to turn the method into a required one in a future major version.

---

## Example 11: Unsafe trait + unsafe impl

```rust
// `Send` is unsafe to implement: the compiler cannot verify your invariants.
pub struct RawHandle { ptr: *mut u8 }

// SAFETY: the underlying allocation is only freed in Drop, never aliased
// elsewhere, and never accessed mutably from multiple threads at once.
unsafe impl Send for RawHandle {}
```

The `unsafe` keyword on the trait `Send` (and on `Sync`, `GlobalAlloc`, etc.) is what forces the implementor to write `unsafe impl`. Without the SAFETY comment, code review cannot validate the claim.
