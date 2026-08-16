# Methods and signatures: trait declaration, supertraits, marker traits, derive

Verbatim signatures from official documentation, with the conditions under which each may or must be used.

---

## Trait declaration syntax (Reference, verbatim)

```text
unsafe? trait IDENTIFIER GenericParams? ( : TypeParamBounds? )? WhereClause? {
    InnerAttribute*
    AssociatedItem*
}
```

[Source: Rust Reference: items/traits](https://doc.rust-lang.org/reference/items/traits.html)

Associated items are:

- **Associated function**: `fn name(...) -> RetType;` (required) or `fn name(...) -> RetType { body }` (provided).
- **Associated type**: `type Item;` (required) or `type Item = DefaultType;` (provided).
- **Associated const**: `const N: u32;` (required) or `const N: u32 = 0;` (provided).

The `unsafe` keyword before `trait` marks the trait itself as unsafe to **implement**: any `impl` for it must be `unsafe impl`. Examples in std: `Send`, `Sync`, `GlobalAlloc`.

---

## Trait impl syntax (Reference)

```text
impl GenericParams? TypePath FOR Type WhereClause? {
    InnerAttribute*
    AssociatedItem*
}
```

[Source: Rust Reference: items/implementations](https://doc.rust-lang.org/reference/items/implementations.html)

Two forms of `impl`:

- **Inherent impl**: `impl<T> MyType<T> { ... }` (no `for`). Methods belong to the type itself.
- **Trait impl**: `impl<T> SomeTrait for MyType<T> { ... }` (has `for`). Methods belong to the trait when applied to that type.

A type may have any number of inherent and trait impls. Each `(Trait, Type)` pair may have at most one impl (coherence).

---

## `std::marker::Send`

```rust
pub unsafe auto trait Send { }
```

[Source: std::marker::Send](https://doc.rust-lang.org/std/marker/trait.Send.html)

Key facts:

- Auto trait: implemented automatically by the compiler when every field is `Send`.
- Unsafe to implement manually: `unsafe impl Send for T {}` asserts thread-safety properties the compiler cannot verify.
- Opt-out: include a `PhantomData<*mut ()>` field, or use a negative impl in a nightly crate.

---

## `std::marker::Sync`

```rust
pub unsafe auto trait Sync { }
```

[Source: std::marker::Sync](https://doc.rust-lang.org/std/marker/trait.Sync.html)

`T: Sync` if and only if `&T: Send`. Otherwise the rules mirror `Send`.

---

## `std::marker::Sized`

```rust
#[lang = "sized"]
pub trait Sized { }
```

[Source: std::marker::Sized](https://doc.rust-lang.org/std/marker/trait.Sized.html)

- Cannot be implemented manually; the compiler does it.
- Every type parameter `T` carries an implicit `T: Sized` bound unless relaxed with `T: ?Sized`.
- `Self` inside a trait body does **not** carry the implicit bound; that is why `dyn Trait` is possible.

---

## `std::marker::Copy`

```rust
pub trait Copy: Clone { }
```

[Source: std::marker::Copy](https://doc.rust-lang.org/std/marker/trait.Copy.html)

- Marker trait with no methods.
- Supertrait `Clone` is required: every `Copy` type is also `Clone`.
- Cannot be implemented for a type that implements `Drop` (E0184).
- Cannot be implemented if any field is not `Copy` (E0204).

---

## `std::marker::Unpin`

```rust
pub auto trait Unpin { }
```

[Source: std::marker::Unpin](https://doc.rust-lang.org/std/marker/trait.Unpin.html)

- Auto trait.
- Opt out by storing `std::marker::PhantomPinned` in a field; the compiler then synthesises `!Unpin`.

---

## Supertrait syntax

Two equivalent forms (Reference):

```rust
trait Circle: Shape { fn radius(&self) -> f64; }

trait Circle where Self: Shape { fn radius(&self) -> f64; }
```

[Source: Rust Reference: items/traits](https://doc.rust-lang.org/reference/items/traits.html)

> "Supertraits are traits that are required to be implemented for a type to implement a specific trait."
> "Anywhere a generic or trait object is bounded by a trait, it has access to the associated items of its supertraits."

Multiple supertraits combine with `+`:

```rust
trait Persisted: Clone + std::fmt::Debug + Send + 'static { /* ... */ }
```

---

## Sealed trait pattern (idiom, not language feature)

```rust
pub trait MyTrait: private::Sealed { /* ... */ }

mod private {
    pub trait Sealed {}
}
```

[Source: Rust API Guidelines: future-proofing](https://rust-lang.github.io/api-guidelines/future-proofing.html#sealed-traits-protect-against-downstream-implementations-c-sealed)

Properties:

- `private::Sealed` is not nameable outside the crate, so no external `impl private::Sealed for X` can exist.
- Therefore no external `impl MyTrait for X` can compile (supertrait must be satisfied).
- Allows adding required methods to `MyTrait` in a minor version without breaking downstream callers.

---

## `#[derive(...)]` for std traits

```text
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
struct Example { /* ... */ }
```

[Source: Rust Reference: attributes/derive](https://doc.rust-lang.org/reference/attributes/derive.html)

Std derives and what each generates:

| Derive | Generated impl |
|--------|----------------|
| `Copy` | `impl Copy for Self {}` (requires `Clone` derive too) |
| `Clone` | `fn clone(&self) -> Self` field-by-field; for `Copy` types it is `*self` |
| `Debug` | `fmt(&self, f: &mut Formatter<'_>) -> fmt::Result` printing struct name + fields |
| `PartialEq` | `fn eq(&self, other: &Self) -> bool` AND-ing field-wise `==` |
| `Eq` | Marker only; asserts `PartialEq` is reflexive |
| `Hash` | `fn hash<H: Hasher>(&self, h: &mut H)` hashing each field in declaration order |
| `Default` | `fn default() -> Self` with `Default::default()` per field |
| `PartialOrd` | `fn partial_cmp(...)` lexicographic on field declaration order |
| `Ord` | `fn cmp(...)` lexicographic; requires `Eq` |

Constraints:

- Every derive imposes its own bound on every field: deriving `Hash` requires all fields `Hash`, deriving `Eq` requires all fields `Eq`, etc.
- For generic types, the derive emits the bound as `where T: Trait`. This can be overly strict (the "perfect derive" problem); manual impls are sometimes warranted.

---

## Orphan rule (Reference, verbatim)

> "A trait implementation is only allowed if either the trait or at least one of the types in the implementation is defined in the current crate."

[Source: Rust Reference: items/implementations](https://doc.rust-lang.org/reference/items/implementations.html)

Errors enforcing this rule:

- **E0117** "only traits defined in the current crate can be implemented for arbitrary types"
- **E0210** "type parameter must be used as the type parameter for some local type"

Workaround: the newtype wrapper. Define a local `struct Wrapper(ForeignType);` and implement the foreign trait for `Wrapper`.

---

## Coherence (Reference)

> "Implementations are restricted to follow trait coherence rules ... two implementations of the same trait cannot apply to the same type."

[Source: Rust Reference: items/implementations](https://doc.rust-lang.org/reference/items/implementations.html)

Error enforcing this rule: **E0119** "conflicting implementations of trait".

Cases that trigger E0119:

- A blanket impl `impl<T> Trait for T` plus a specific impl `impl Trait for u32`.
- Two blanket impls with bounds that can both apply to the same `T`.
- An auto-derive in a re-export crate plus a manual impl downstream.

---

## Fully qualified path syntax for trait method calls

```rust
trait Animal { fn name() -> &'static str; }
trait Dog    { fn name() -> &'static str; }

struct Spot;
impl Animal for Spot { fn name() -> &'static str { "animal-spot" } }
impl Dog    for Spot { fn name() -> &'static str { "dog-spot" } }

let a = <Spot as Animal>::name();   // "animal-spot"
let d = <Spot as Dog>::name();      // "dog-spot"
```

[Source: Rust Book Ch 19.3: Advanced Traits](https://doc.rust-lang.org/book/ch19-03-advanced-traits.html)

Use the `<Type as Trait>::method` form to disambiguate when:

- Two traits in scope provide methods with the same name.
- An inherent method shadows a trait method.
- The method is an associated function (no `self`) so receiver-based dispatch cannot disambiguate.
