---
name: rust-syntax-traits
description: >
  Use when the user defines or implements a trait, asks about default methods / supertraits / marker traits / blanket impls / sealed traits, encounters orphan-rule errors, or needs to choose between inherent impl and trait impl.
  Prevents orphan-rule violations, accidentally exposing an extensible trait users can implement, and confusing inherent vs trait methods.
  Covers: trait definition syntax (required + default methods), supertraits, marker traits (Send / Sync / Sized / Copy / Unpin), blanket impls, sealed trait pattern, inherent vs trait impl, orphan rule + coherence, derive macros (Copy / Clone / Debug / PartialEq / Eq / Hash / Default).
  Keywords: trait, "trait definition", "default method", supertrait, "marker trait", "blanket impl", "sealed trait", "orphan rule", coherence, "inherent impl", "impl Trait for Type", "newtype wrapper", "cannot impl foreign trait", derive, "#[derive]", "method not found", E0117, E0119, E0120, E0210, "what does trait do", "how to add method".
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# rust-syntax-traits

The **mechanics** of trait declaration and implementation in Rust: how to define a trait, how default methods work, how supertraits compose, how the orphan rule and coherence constrain `impl Trait for Type`, how to seal a trait against external implementors, and which built-in marker traits (`Send`, `Sync`, `Sized`, `Copy`, `Unpin`) have special compiler treatment.

Cross-references: [[rust-syntax-generics]] (trait bounds on type parameters) [[rust-syntax-trait-objects]] (`dyn Trait` and dyn-compatibility) [[rust-syntax-gats]] (associated types with generics) [[rust-errors-trait-bounds]] (E-codes for unsatisfied bounds) [[rust-core-memory-model]] (Send / Sync auto-trait rules)

---

## When to use this skill

- User writes `trait Foo { ... }` or `impl Trait for Type { ... }` and needs the rules
- User hits an orphan-rule error (E0117) or coherence error (E0119) and asks how to fix it
- User needs to add a method to a trait without breaking downstream implementors
- User asks about `Send` / `Sync` / `Sized` / `Copy` / `Unpin` and how to opt in or out
- User wants to write an extensible-looking trait that only the defining crate may implement (sealed trait)
- User confuses inherent methods (`impl Foo`) with trait methods (`impl Trait for Foo`)
- User asks about `#[derive(...)]` and which derives are available in `std`

For `dyn Trait` and object safety see [[rust-syntax-trait-objects]]. For generic-parameter trait bounds (`fn foo<T: Trait>()`) see [[rust-syntax-generics]].

---

## Trait definition syntax (Reference, verbatim)

```text
unsafe? trait IDENTIFIER GenericParams? ( : TypeParamBounds? )? WhereClause? {
    InnerAttribute*
    AssociatedItem*
}
```

[Source: Rust Reference: items/traits][ref-traits]

A trait body consists of associated items:

```rust
trait Example {
    const CONST_NO_DEFAULT: i32;
    const CONST_WITH_DEFAULT: i32 = 99;
    type TypeNoDefault;
    fn method_without_default(&self);
    fn method_with_default(&self) {}
}
```

Each associated item is either **required** (no body) or **provided** (has a body / default value). Implementors must provide every required item; they MAY override any provided one.

ALWAYS prefer a provided (default) method when the behaviour can be expressed in terms of other trait methods. This shields downstream implementors from breakage when you later refine the default.

NEVER add a new required method to a published trait: it is a breaking change. Add a method WITH a default body instead.

---

## Quick reference table

| Concept | Syntax | Compiler rule |
|---------|--------|---------------|
| Required method | `fn m(&self) -> T;` | Implementors MUST provide a body |
| Default method | `fn m(&self) -> T { ... }` | Implementors MAY override |
| Associated type | `type Item;` | One concrete `Item` per impl |
| Associated const | `const N: u32;` (or `= 0`) | One value per impl |
| Supertrait | `trait Sub: Super { ... }` | Implementor of `Sub` MUST also impl `Super` |
| Marker trait | `trait Marker {}` (often `unsafe trait`) | No items, used purely as a tag |
| Blanket impl | `impl<T: A> B for T { ... }` | Applies to every `T` that satisfies the bound |
| Sealed trait | private supertrait | Outsiders cannot implement |
| Inherent impl | `impl Foo { ... }` | Methods belong to `Foo` directly |
| Trait impl | `impl Trait for Foo { ... }` | Methods belong to `Trait` for `Foo` |
| Orphan rule (E0117) | local trait OR local type | Foreign-trait-for-foreign-type rejected |
| Coherence (E0119) | one impl per (Trait, Type) | Overlapping impls rejected |

---

## Required vs default (provided) methods

```rust
pub trait Greeter {
    // Required: every impl must supply this.
    fn name(&self) -> &str;

    // Provided: callers get this for free, impls may override.
    fn greet(&self) -> String {
        format!("Hello, {}!", self.name())
    }
}

struct World;
impl Greeter for World {
    fn name(&self) -> &str { "world" }
    // greet() uses the default.
}
```

[Source: Rust Reference: items/traits][ref-traits] [Source: Rust Book Ch 10.2: Traits][book-traits]

ALWAYS write defaults in terms of required methods so the contract stays composable. NEVER let a default method call itself directly; that compiles but recurses at runtime.

---

## Supertraits

A supertrait is a trait bound on the trait itself. Two equivalent forms:

```rust
trait Circle: Shape { fn radius(&self) -> f64; }

trait Circle where Self: Shape { fn radius(&self) -> f64; }
```

> "Supertraits are traits that are required to be implemented for a type to implement a specific trait."
> "Anywhere a generic or trait object is bounded by a trait, it has access to the associated items of its supertraits."
> [Source: Rust Reference: items/traits][ref-traits]

Implications:

- Any type that implements `Circle` MUST also implement `Shape`.
- Inside `Circle`'s default methods and inside `dyn Circle` you can call `Shape`'s methods directly.
- Removing a supertrait is a breaking change; adding one is too unless every existing implementor already happens to implement it.

ALWAYS list a supertrait when the subtrait genuinely needs the supertrait's items. NEVER use a supertrait as a "convenience bound" you don't actually rely on; it locks downstream code into implementing it too.

---

## Marker traits (Send / Sync / Sized / Copy / Unpin)

A marker trait has no items. The compiler gives several of them special meaning. From the Reference's `special-types-and-traits` page:

| Trait | Meaning | How it is implemented |
|-------|---------|-----------------------|
| `Sized` | "The size of this type is known at compile-time." Implicit bound on every type parameter unless relaxed with `?Sized`. | Automatic; cannot be implemented manually. |
| `Copy` | Bit-for-bit duplicable on assignment. Requires `Clone` as supertrait. | `#[derive(Copy, Clone)]` (or manual). All fields MUST be `Copy`. A `Drop` impl excludes `Copy` (E0184). |
| `Send` | Safe to move to another thread. | **Auto trait**: derived structurally from field types. `unsafe impl Send for T {}` to assert manually. |
| `Sync` | Safe to share `&T` across threads. | Auto trait, same rules as `Send`. |
| `Unpin` | Safe to move out of a `Pin<&mut T>`. | Auto trait. Opt out by storing `PhantomPinned` in a field. |

[Source: Rust Reference: special-types-and-traits][ref-special]

### Auto traits (Send / Sync / Unpin)

An auto trait is implemented for a composite type if and only if every field implements it. The compiler does this without any `impl` block.

ALWAYS think of `Send`, `Sync`, and `Unpin` as **structural**: change a field type and the whole struct's `Send`-ness can flip.

NEVER write `unsafe impl Send for T {}` without a soundness comment explaining why every field can in fact be moved across threads. Use a `PhantomData<*mut ()>` field to OPT OUT of `Send`/`Sync` (the raw-pointer trick).

### Sized and ?Sized

```rust
fn takes_sized<T>(_: T) {}              // Implicit T: Sized
fn takes_unsized<T: ?Sized>(_: &T) {}   // Accepts str, [u8], dyn Trait
```

The Reference is explicit: "Type parameters (except `Self` in traits) are `Sized` by default, as are associated types. These implicit `Sized` bounds may be relaxed by using the special `?Sized` bound." [Source: Rust Reference: special-types-and-traits][ref-special]

`?Sized` MAY appear only on type parameters and `Self`. It MUST never be combined with a value receiver (a `?Sized` value cannot live on the stack).

---

## Blanket impls

A blanket impl uses a generic parameter for the implementing type:

```rust
pub trait Stringify {
    fn stringify(&self) -> String;
}

// Blanket: every Display type automatically gets Stringify.
impl<T: std::fmt::Display> Stringify for T {
    fn stringify(&self) -> String { format!("{self}") }
}
```

Constraints:

- A blanket impl rules out any further `impl Stringify for SomeType` for types matching the bound. Coherence (E0119) makes this an error.
- Adding a NEW blanket impl to a published trait is a breaking change: it can conflict with downstream `impl`s.
- The classic stdlib example is `impl<T: Clone> ToOwned for T`.

ALWAYS reserve blanket impls for "default behaviour for all `T: Bound`" semantics. NEVER ship a blanket impl alongside a contradictory per-type impl: coherence will reject one and your API will need a redesign.

---

## Sealed trait pattern

The orphan rule (below) lets crates outside yours implement a `pub` trait you publish for their own types. If you do NOT want that, seal the trait: declare it as a subtrait of a PRIVATE supertrait. Only your crate can implement the private supertrait, so only your crate can implement the public trait.

```rust
pub trait Operation: private::Sealed {
    fn run(&self) -> u32;
}

mod private {
    pub trait Sealed {}     // private: not nameable outside the crate
}

pub struct Add;
impl private::Sealed for Add {}
impl Operation for Add { fn run(&self) -> u32 { 1 + 2 } }
```

Properties:

- External crates can `use Operation` and CALL `op.run()`.
- External crates CANNOT write `impl Operation for MyType` (E0445 / E0446 on the private supertrait).
- You retain freedom to add required methods to `Operation` without breaking downstream callers, because they cannot have implementors.

ALWAYS seal a trait whose method set you might extend in a non-major release. NEVER seal a trait whose entire purpose is downstream extension (e.g. `serde::Serialize`); that defeats the contract.

[Source: Rust API Guidelines: future-proofing][api-guidelines]

---

## Inherent impl vs trait impl

```rust
struct Counter { n: u32 }

// Inherent impl: methods belong to Counter itself.
impl Counter {
    fn new() -> Self { Self { n: 0 } }
    fn tick(&mut self) { self.n += 1; }
}

// Trait impl: methods belong to Iterator for Counter.
impl Iterator for Counter {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        self.tick();
        Some(self.n)
    }
}
```

Rules:

- A type may have any number of inherent `impl` blocks AND any number of trait `impl` blocks.
- Inherent methods take precedence in method resolution; trait methods are reached only when the trait is in scope.
- An inherent method MAY shadow a trait method of the same name. The trait method is then callable only via fully qualified path: `Trait::method(&counter)`.

ALWAYS write `Iterator::next(&mut c)` (or import `Iterator` and call `c.next()`) when a trait method is shadowed. NEVER assume a trait method "just works" without importing the trait or relying on the prelude.

[Source: Rust Reference: items/implementations][ref-impl]

---

## Orphan rule and coherence

> "A trait implementation is only allowed if either the trait or at least one of the types in the implementation is defined in the current crate."
> [Source: Rust Reference: items/implementations][ref-impl]

This is the **orphan rule** (E0117). It exists so that adding a dependency cannot retroactively change which `impl` your code resolves to.

| Trait location | First type parameter location | Allowed? |
|----------------|-------------------------------|----------|
| Local crate | Local or foreign | YES |
| Foreign crate | Local | YES |
| Foreign crate | Foreign | NO (E0117) |

The fix for the forbidden case is the **newtype wrapper**:

```rust
// Forbidden: impl Display for Vec<u8> { ... }  // both foreign, E0117

pub struct Bytes(pub Vec<u8>);  // local type wrapping the foreign one
impl std::fmt::Display for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in &self.0 { write!(f, "{b:02x}")?; }
        Ok(())
    }
}
```

**Coherence** (E0119) is the sibling rule: at most one impl of a given trait may apply to a given concrete type. Overlapping blanket impls and a blanket-plus-specific combination both fail E0119.

ALWAYS wrap a foreign type in a local newtype when you need to implement a foreign trait for it. NEVER try to add an `impl ForeignTrait for ForeignType` even via a re-export trick: the orphan rule is checked on the concrete crate identity, not the import path.

[Source: Rust Reference: items/implementations][ref-impl]

---

## Decision tree: trait impl vs inherent impl

```
Need to attach behaviour to a type?
|
+-- Will the behaviour be used through a generic bound or dyn Trait?
|   |
|   +-- YES -> trait impl (impl Trait for Type)
|   |          Required when the method must be invoked polymorphically.
|   |
|   +-- NO  -> step deeper
|
+-- Is there an existing trait in std or a dep whose contract fits exactly
|   (Display, From, Iterator, IntoIterator, Default, AsRef<T>, etc.)?
|   |
|   +-- YES -> trait impl for that existing trait.
|              ALWAYS prefer matching a standard trait over a custom one.
|   |
|   +-- NO  -> step deeper
|
+-- Is the behaviour purely type-specific, with no polymorphism in sight?
    |
    +-- YES -> inherent impl (impl Type { ... })
    +-- NO  -> design a new trait + trait impl.
```

ALWAYS implement standard traits when their contract matches the operation (`Display`, `From`, `Default`, ...). NEVER invent a custom trait that duplicates an existing std contract: callers will be unable to plug your type into generic code.

---

## Derive macros for built-in traits

`#[derive(...)]` is sugar that asks the compiler to synthesise an implementation. The std-supported derives:

| Trait | Behaviour produced by the derive | Requires |
|-------|-----------------------------------|----------|
| `Copy` | Marker only (no method). | `Clone` MUST be in the same derive. All fields `Copy`. No `Drop`. |
| `Clone` | Field-by-field clone. | All fields `Clone`. |
| `Debug` | Struct/enum dump in `{:?}` formatter. | All fields `Debug`. |
| `PartialEq` | Field-by-field `==`. | All fields `PartialEq`. |
| `Eq` | Marker only (asserts reflexive equality). | All fields `Eq`. |
| `Hash` | Field-by-field hash. | All fields `Hash`. |
| `Default` | Calls `Default::default()` per field. | All fields `Default`. |
| `PartialOrd` | Lexicographic on field order. | All fields `PartialOrd`. |
| `Ord` | Lexicographic on field order. | All fields `Ord` and `Eq`. |

[Source: Rust Reference: derive][ref-derive]

ALWAYS group derives in declaration order: `#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]`. NEVER derive `Copy` without `Clone` (E0204): every `Copy` type is `Clone`.

ALWAYS think about whether the derive's structural definition is the contract you want. The classic mistake: `#[derive(PartialEq)]` on a type with a cached hash field will compare the cache too, producing false negatives.

---

## Common error codes

| Code | Trigger | Fix |
|------|---------|-----|
| E0117 | Orphan rule: foreign trait + foreign type | Wrap one of them in a local newtype |
| E0119 | Coherence: two overlapping `impl`s | Make the impls non-overlapping or remove one |
| E0120 | `Drop` impl for type that is not a struct/enum/union | Implement `Drop` only on a concrete owned type |
| E0184 | `#[derive(Copy)]` on a type with `Drop` impl | Drop OR Copy, never both |
| E0204 | `#[derive(Copy)]` and a field is not `Copy` | Remove `Copy`, keep `Clone` |
| E0210 | Type parameter must be used as the first local type | Reorder generics so a local type appears first |
| E0277 | Trait bound not satisfied | Add an `impl`, add a bound, or change the type |
| E0599 | Method not found (trait not in scope) | `use Trait;` to bring the trait into scope |

Full E-code drilldown lives in [[rust-errors-trait-bounds]].

---

## Section: Avoid these mistakes

(Full list with WHYs in `references/anti-patterns.md`.)

- Trying to `impl ForeignTrait for ForeignType` (orphan rule, E0117). Fix: newtype wrapper.
- Deriving `Copy` without `Clone` in the same attribute (E0204). Both are required together.
- Implementing `Drop` AND deriving `Copy` on the same type (E0184). They are mutually exclusive by design.
- Publishing an extensible-looking `pub trait` without sealing it, then trying to add a required method later (breaking change). Seal first, extend safely later.
- Using an inherent `impl` when the method must be invoked through `dyn` or a generic `T: Trait` bound. Inherent methods are not part of any trait and cannot be dispatched polymorphically.
- Adding a new required method to a published trait. ALWAYS add it with a default body or behind a sealed trait.
- Writing `unsafe impl Send for T {}` without a soundness comment. Reviewers cannot verify the claim and the compiler will not catch a mistake.

---

## Reference links

[ref-traits]: https://doc.rust-lang.org/reference/items/traits.html
[ref-impl]: https://doc.rust-lang.org/reference/items/implementations.html
[ref-special]: https://doc.rust-lang.org/reference/special-types-and-traits.html
[ref-derive]: https://doc.rust-lang.org/reference/attributes/derive.html
[book-traits]: https://doc.rust-lang.org/book/ch10-02-traits.html
[api-guidelines]: https://rust-lang.github.io/api-guidelines/future-proofing.html
[send-trait]: https://doc.rust-lang.org/std/marker/trait.Send.html
[sync-trait]: https://doc.rust-lang.org/std/marker/trait.Sync.html
[sized-trait]: https://doc.rust-lang.org/std/marker/trait.Sized.html
[copy-trait]: https://doc.rust-lang.org/std/marker/trait.Copy.html
[unpin-trait]: https://doc.rust-lang.org/std/marker/trait.Unpin.html

- [Rust Reference: items/traits][ref-traits]
- [Rust Reference: items/implementations][ref-impl]
- [Rust Reference: special-types-and-traits][ref-special]
- [Rust Reference: attributes/derive][ref-derive]
- [Rust Book Ch 10.2: Traits: Defining Shared Behaviour][book-traits]
- [Rust API Guidelines: future-proofing (sealed traits)][api-guidelines]
- [`std::marker::Send`][send-trait]
- [`std::marker::Sync`][sync-trait]
- [`std::marker::Sized`][sized-trait]
- [`std::marker::Copy`][copy-trait]
- [`std::marker::Unpin`][unpin-trait]

For deeper drill-downs see:
- `references/methods.md`: trait, supertrait, derive, and marker-trait signatures verbatim from the docs.
- `references/examples.md`: complete working examples for default methods, supertraits, blanket impls, sealed traits, newtype wrappers, and derives.
- `references/anti-patterns.md`: the most common trait-design and impl mistakes with root-cause explanations and fixes.
