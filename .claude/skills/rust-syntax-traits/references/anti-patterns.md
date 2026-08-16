# Anti-patterns: trait and impl mistakes

Each entry: a concrete mistake, WHY it fails (root cause), and the correct alternative.

---

## 1. `impl ForeignTrait for ForeignType` (orphan-rule violation, E0117)

```rust
// In your own crate:
impl std::fmt::Display for Vec<u8> {   // E0117
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }
}
```

**Why it fails.** The orphan rule says: "a trait implementation is only allowed if either the trait or at least one of the types in the implementation is defined in the current crate." Both `Display` and `Vec<u8>` are foreign. If this were allowed, another crate could write a different `impl Display for Vec<u8>` and merging the two dependency trees would yield ambiguous method resolution, silently changing program behaviour on a dependency update.

**Fix.** Use the newtype wrapper:

```rust
pub struct Hex(pub Vec<u8>);
impl std::fmt::Display for Hex { /* ... */ }
```

Now `Hex` is local, the orphan rule is satisfied, and method resolution is unambiguous.

---

## 2. `#[derive(Copy)]` without `Clone` (E0204)

```rust
#[derive(Copy)]      // E0204: the trait `Copy` cannot be implemented
struct Point { x: i32, y: i32 }
```

**Why it fails.** `Copy: Clone`. Every `Copy` type must also be `Clone`. The derive macro for `Copy` does not synthesise a `Clone` impl; you must derive `Clone` too.

**Fix.**

```rust
#[derive(Copy, Clone)]
struct Point { x: i32, y: i32 }
```

ALWAYS group the two derives together: `#[derive(Copy, Clone, ...)]`.

---

## 3. `impl Drop` on a type that also derives `Copy` (E0184)

```rust
#[derive(Copy, Clone)]
struct Token(u32);

impl Drop for Token {    // E0184: cannot implement Copy and Drop on the same type
    fn drop(&mut self) { println!("dropping {}", self.0); }
}
```

**Why it fails.** `Copy` says "duplicating this value is a no-op memcpy". `Drop` says "destruction runs custom code". The two are contradictory: if you `let b = a;` on a `Copy` type, the original `a` is still live and would also be dropped, leading to a double-`drop` of any owned resource the type holds. The compiler rejects the combination at definition time.

**Fix.** Choose one:

- Remove `Copy` and use `.clone()` (or restructure to move) when duplication is needed.
- Remove `Drop` and use RAII via wrapper types (`Box<T>`, `Vec<T>`, etc.) that already implement it.

---

## 4. Publishing an extensible `pub trait` without sealing it

```rust
// crate v1.0:
pub trait Storage {
    fn load(&self, key: &str) -> Option<Vec<u8>>;
}

// crate v1.1 wants to add a required method:
pub trait Storage {
    fn load(&self, key: &str) -> Option<Vec<u8>>;
    fn save(&self, key: &str, val: &[u8]);   // breaking change!
}
```

**Why it fails.** Any downstream crate that wrote `impl Storage for Db {}` against v1.0 now stops compiling against v1.1: `save` is required and not provided. This is a semver-major change and a serious source of ecosystem breakage.

**Fix.** Either:

- (a) Seal the trait from the start so only your crate implements it. Then adding required methods is safe.
- (b) Add new methods with a provided default body. Existing implementors automatically inherit the default.

```rust
pub trait Storage: private::Sealed {
    fn load(&self, key: &str) -> Option<Vec<u8>>;
    fn save(&self, key: &str, val: &[u8]) { /* default no-op */ }
}
mod private { pub trait Sealed {} }
```

---

## 5. Using inherent `impl` when polymorphism is required

```rust
struct PrettyPrinter;

impl PrettyPrinter {           // inherent impl
    fn fmt(&self, x: &dyn std::fmt::Debug) -> String { format!("{x:?}") }
}

fn render<T: std::fmt::Display>(_: &T) {}

// User tries:
render(&PrettyPrinter);        // E0277: PrettyPrinter does not implement Display
```

**Why it fails.** Inherent methods belong to the concrete type and are NOT part of any trait. Generic code parameterised on `T: Display` and trait objects `&dyn Display` will not see them. Method-resolution looks for an `impl Display for PrettyPrinter`, finds none, and rejects the call.

**Fix.** Implement the relevant trait:

```rust
impl std::fmt::Display for PrettyPrinter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<printer>")
    }
}
```

ALWAYS map the operation to an existing standard trait (`Display`, `From`, `Default`, `AsRef<T>`) when one fits. Custom traits are for behaviours std does not already model.

---

## 6. Adding a new required method to a published trait

```rust
// crate v1.0:
pub trait Codec {
    fn encode(&self, data: &[u8]) -> Vec<u8>;
}

// crate v1.1:
pub trait Codec {
    fn encode(&self, data: &[u8]) -> Vec<u8>;
    fn decode(&self, data: &[u8]) -> Vec<u8>;   // BREAKING
}
```

**Why it fails.** Same as anti-pattern 4 but worth calling out separately: adding a method without a default is ALWAYS a breaking change for any unsealed trait. The compiler does not synthesise method bodies; existing impls have nothing to dispatch to, and the build breaks.

**Fix.** Provide a default body:

```rust
pub trait Codec {
    fn encode(&self, data: &[u8]) -> Vec<u8>;
    fn decode(&self, data: &[u8]) -> Vec<u8> {
        Vec::new()    // sane default; implementors may override
    }
}
```

If no useful default exists, gate the new method behind a NEW trait (`pub trait CodecExt: Codec { fn decode(...); }`) so existing `Codec` impls remain valid.

---

## 7. `unsafe impl Send for T {}` without a SAFETY comment

```rust
pub struct RawBuffer { ptr: *mut u8, len: usize }

unsafe impl Send for RawBuffer {}    // no safety justification, review red flag
```

**Why it fails.** `Send` is an `unsafe auto trait`: the compiler refuses to derive it because the struct contains a raw pointer (which is `!Send` by default). Writing `unsafe impl` asserts you have proved the type can be moved between threads safely, but without a SAFETY comment a reviewer cannot evaluate the claim, and a future code change that breaks the invariant (e.g. someone aliases the pointer) will go unnoticed.

**Fix.** Either restructure to remove the raw pointer (use `Box<[u8]>`), or write a SAFETY comment that explains the proof:

```rust
// SAFETY: `ptr` points to a heap allocation we exclusively own.
// We never share the pointer with another thread while this RawBuffer is live;
// Drop frees the allocation; no internal aliasing exists.
unsafe impl Send for RawBuffer {}
```

The Rust standard library and Clippy both enforce this pattern: `clippy::missing_safety_doc` lints on missing SAFETY comments for `unsafe impl`.

---

## 8. Trait method invisible because the trait is not in scope (E0599)

```rust
// crate `foo` exports:
pub trait FooExt { fn extra(&self) -> u32; }
impl FooExt for i32 { fn extra(&self) -> u32 { 42 } }

// In a consumer file:
fn main() {
    let _x = 5_i32.extra();   // E0599: no method named `extra` found
}
```

**Why it fails.** Method resolution only considers traits that are in scope. The `FooExt` impl exists but the consumer file did not `use foo::FooExt`, so the method lookup fails.

**Fix.**

```rust
use foo::FooExt;
fn main() {
    let _x = 5_i32.extra();   // ok
}
```

Library authors sometimes publish a `prelude` module re-exporting all extension traits so users can `use foo::prelude::*;` once.

---

## 9. Two blanket impls colliding (E0119)

```rust
trait Convert {
    fn convert(self) -> String;
}

// Blanket 1: every Display type.
impl<T: std::fmt::Display> Convert for T {
    fn convert(self) -> String { format!("{self}") }
}

// Blanket 2: every Debug type.
impl<T: std::fmt::Debug> Convert for T {    // E0119: conflicting impls
    fn convert(self) -> String { format!("{self:?}") }
}
```

**Why it fails.** Any `T` that is both `Display` and `Debug` (a very common case: `u32`, `String`, ...) matches both blankets. Coherence forbids ambiguity. The compiler refuses to pick one and rejects the second impl.

**Fix.** Either:

- Make the second blanket non-overlapping by adding a negative-trait-like distinguishing bound (rarely possible on stable).
- Replace one blanket with concrete impls.
- Drop one of the blankets and provide a free helper function for the other formatting style.

---

## 10. Deriving `PartialEq` on a type with a "cache" field

```rust
#[derive(PartialEq, Eq, Hash)]
struct CachedString {
    text: String,
    cached_hash: u64,    // computed lazily, ignored conceptually
}
```

**Why it fails.** `#[derive(PartialEq)]` produces a STRUCTURAL equality check: it AND-s `==` over every field. Two `CachedString` values with the same `text` but different cached hashes will compare unequal, which violates the user-visible semantics and breaks `HashMap` / `HashSet` lookup.

**Fix.** Hand-write the impls so they reflect the semantic identity:

```rust
struct CachedString { text: String, cached_hash: u64 }
impl PartialEq for CachedString {
    fn eq(&self, other: &Self) -> bool { self.text == other.text }
}
impl Eq for CachedString {}
impl std::hash::Hash for CachedString {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) { self.text.hash(h); }
}
```

ALWAYS audit derived `PartialEq` / `Eq` / `Hash` / `PartialOrd` / `Ord` impls when the struct contains computed, cached, or external fields. The derive is structural, your contract may not be.
