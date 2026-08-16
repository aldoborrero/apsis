---
name: rust-syntax-lifetimes
description: >
  Use when the user must write a lifetime annotation, encounters E0106 / E0623 / E0495 / E0700 / E0759, needs to understand the three elision rules, the 'static bound vs &'static T, HRTB `for<'a>`, lifetime subtyping, variance (covariant / contravariant / invariant), or edition-2024 RPIT precise-capturing.
  Prevents over-annotating, confusing `T: 'static` (no internal short borrow) with `&'static T` (reference lives for program), or missing variance-related compile failures.
  Covers: explicit annotations, three elision rules, `'static` two senses, HRTB, lifetime subtyping (`'a: 'b`), variance, NLL behaviour reminder, edition-2024 RPIT capture (`+ use<'a, T>`).
  Keywords: lifetime, "'a", "lifetime annotation", "missing lifetime", "lifetime elision", "elision rule", "for<'a>", HRTB, "higher-rank trait bound", "'static", "static bound", variance, covariance, contravariance, invariance, PhantomData lifetime, E0106, E0623, E0495, E0700, E0759, "hidden lifetime", "precise capturing", "use<>", "doesn't live long enough", "outlives".
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# rust-syntax-lifetimes

Lifetimes are compile-time-only annotations describing the validity scope of references. The compiler ALWAYS infers lifetimes for local variables; explicit annotations are required ONLY where the borrow checker cannot deduce relationships between input and output references on function and type signatures. This skill covers when annotation is required, how elision works, the two distinct meanings of `'static`, HRTB, variance, and the edition-2024 RPIT capture change.

## Quick Reference

### Core rules (deterministic)

1. ALWAYS rely on lifetime elision in function signatures when it applies. NEVER write `<'a>` if the three elision rules already infer the same signature (Clippy lint `needless_lifetimes`).
2. NEVER use `T: 'static` when a generic `'a` would suffice. The `'static` bound overconstrains callers and is one of the most common over-annotation mistakes.
3. NEVER confuse `&'static T` (a reference that lives the whole program) with `T: 'static` (a bound saying the value contains no non-`'static` borrows; owned types satisfy it).
4. ALWAYS treat `&mut T` as invariant in `T`. NEVER assume it is covariant; passing `&mut Vec<&'static str>` where `&mut Vec<&'a str>` is expected is rejected because of invariance.
5. ALWAYS add `+ use<'a, T>` to an RPIT in edition 2024 when you want to NOT capture a generic; the 2024 default is to capture all in-scope generics, the inverse of pre-2024 behaviour.

### Three elision rules (function signatures only)

Per `https://doc.rust-lang.org/reference/lifetime-elision.html`:

1. Each elided input lifetime in the parameters becomes a distinct lifetime parameter.
2. If there is exactly one input lifetime (elided or not), it is assigned to every elided output lifetime.
3. If the function has a receiver of type `&Self` or `&mut Self`, the lifetime of that receiver is assigned to every elided output lifetime.

Elision applies ONLY to function and method signatures. It NEVER applies to type definitions; structs and enums always require explicit lifetimes.

### Decision tree: "Do I need to write a lifetime?"

```
Is the item a struct, enum, union, or trait holding a reference?
  YES -> ALWAYS write explicit lifetime parameters (no elision in type defs)
  NO  -> Is it a function or method?
    YES -> Apply the three elision rules in order
      Rule 1 covers all input lifetimes? -> elision may still fail for outputs
      Rule 2 (single input) or Rule 3 (self method) covers outputs? -> elide
      Otherwise -> compiler emits E0106; write explicit lifetimes
    NO  -> Lifetimes are inferred (closures, let-bindings); NEVER annotate by hand
```

### Variance cheat-sheet (memorise this table)

| Construct | Variance in `'a` | Variance in `T` |
|-----------|------------------|-----------------|
| `&'a T` | covariant | covariant |
| `&'a mut T` | covariant | invariant |
| `*const T` | n/a | covariant |
| `*mut T` | n/a | invariant |
| `[T]`, `[T; n]` | n/a | covariant |
| `fn() -> T` (return) | n/a | covariant |
| `fn(T) -> ()` (argument) | n/a | contravariant |
| `UnsafeCell<T>`, `Cell<T>`, `RefCell<T>` | n/a | invariant |
| `PhantomData<T>` | n/a | covariant |
| `dyn Trait + 'a` | covariant | n/a |

Source: `https://doc.rust-lang.org/reference/subtyping.html`.

## Pattern: explicit annotation on a struct

When a struct holds a reference, ALWAYS parameterise it over the lifetime of that reference. Elision does NOT apply to type definitions.

```rust
struct Excerpt<'a> {
    part: &'a str,
}

impl<'a> Excerpt<'a> {
    // Elision rule 3: output lifetime is self's
    fn part(&self) -> &str {
        self.part
    }
}
```

The lifetime parameter `'a` on `Excerpt` says: an `Excerpt<'a>` is valid for at most the duration `'a`. The implementation block carries the same parameter.

## Pattern: function with multiple input lifetimes

When two references with potentially different lifetimes must produce an output whose lifetime is one of them, elision rule 2 does NOT apply (more than one input lifetime). The compiler emits E0106; the caller must annotate.

```rust
// NEVER write this; compiler error E0106
// fn longest(x: &str, y: &str) -> &str { if x.len() > y.len() { x } else { y } }

// ALWAYS bind output to a specific input lifetime
fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() { x } else { y }
}
```

The single `'a` parameter unifies both inputs and the output. The caller is forced to pass references that share at least one common lifetime region.

## Pattern: `'static` two senses

```rust
// Sense 1: &'static T -> reference valid for the entire program
let s: &'static str = "literal";   // string literals are &'static str
let leaked: &'static mut [u8; 4] = Box::leak(Box::new([0u8; 4]));

// Sense 2: T: 'static bound -> value contains no borrows shorter than 'static
fn requires_static<T: 'static>(value: T) { /* ... */ }

let owned: String = String::from("owned"); // String: 'static (no borrows inside)
requires_static(owned);                    // OK: String satisfies T: 'static

let local = 42i32;
let r: &i32 = &local;
// requires_static(r); // ERROR: &'_ i32 does NOT satisfy 'static (borrow scope is local)
```

ALWAYS reach for `T: 'static` ONLY when a value will be stored in a place that outlives any caller frame (a static, a `thread::spawn` closure, a long-lived registry). NEVER add it "to make the compiler shut up": it propagates to callers.

## Pattern: HRTB (higher-rank trait bound)

`for<'a>` quantifies a trait bound over ALL lifetimes. Required for closures that must accept a borrow of any lifetime, particularly when stored or passed into a function that does not yet know the lifetime.

```rust
fn apply<F>(f: F) -> String
where
    F: for<'a> Fn(&'a str) -> &'a str,
{
    let s = String::from("hello");
    f(&s).to_owned()
}

apply(|x| x); // closure works for every lifetime
```

NEVER write `F: Fn(&'a str) -> &'a str` at the top level without first introducing `'a` somewhere; either declare `'a` as a generic parameter on the function or use HRTB. HRTB is the canonical form when the closure is generic over the input lifetime.

## Pattern: lifetime subtyping `'a: 'b`

`'a: 'b` reads "`'a` outlives `'b`". It declares that anywhere a `'b` is expected, an `'a` is acceptable, because `'a` lives at least as long.

```rust
struct Pair<'a, 'b: 'a> {
    long: &'b str,
    short: &'a str,
}
```

ALWAYS use lifetime subtyping when storing references of different lifetimes in the same struct and the longer one must be assigned where the shorter is expected.

## Pattern: NLL recap (non-lexical lifetimes)

Borrow scopes end at the LAST USE of the reference, NOT at the closing brace.

```rust
let mut v = vec![1, 2, 3];
let first = &v[0];
println!("{first}");        // last use of `first` here
v.push(4);                  // OK under NLL; pre-NLL this was rejected
```

NEVER manually narrow a borrow with extra `{ }` scopes; rely on NLL. ALWAYS let the compiler determine the end of the borrow region.

## Pattern: edition-2024 RPIT precise capturing

Edition 2024 changed the default capture of return-position `impl Trait`. Per `https://doc.rust-lang.org/edition-guide/rust-2024/rpit-lifetime-capture.html`:

> In Rust 2024, all in-scope generic parameters, including lifetime parameters, are implicitly captured when the `use<..>` bound is not present.

ALWAYS use `+ use<...>` to opt OUT of capturing a generic; this is the inverse of the pre-2024 default.

```rust
// Edition 2024: captures 'a implicitly
fn first_word(s: &str) -> impl Iterator<Item = char> {
    s.chars().take_while(|c| !c.is_whitespace())
}

// To NOT capture the lifetime of T (e.g. for older-API compatibility), add use<>
fn make_iter<T: Clone>(_t: &T) -> impl Iterator<Item = u8> + use<> {
    std::iter::empty()
}

// To capture only specific generics:
fn select<'a, T: Clone>(t: &'a T) -> impl std::fmt::Debug + use<'a, T> {
    (t.clone(),)
}
```

ALWAYS run `cargo fix --edition` to apply the `impl_trait_overcaptures` lint when migrating from a pre-2024 edition: it auto-inserts `+ use<>` where pre-2024 semantics must be preserved.

## Anti-patterns (cross-reference [[rust-syntax-lifetimes]] -> [[rust-errors-lifetimes]])

NEVER do any of the following. Full explanations are in `references/anti-patterns.md`.

1. Annotating lifetimes that elision would already infer (Clippy `needless_lifetimes`).
2. Reaching for `T: 'static` to silence the compiler instead of plumbing a `'a` through.
3. Confusing `T: 'static` (bound) with `&'static T` (reference type).
4. Assuming `&mut T` is covariant in `T` (it is INVARIANT).
5. Returning RPIT in edition 2024 without considering the new default capture; failing to add `+ use<>` when older API stability is required.
6. Holding a borrow longer than its last use by introducing artificial scopes; NLL already ends the borrow.

## Cross-References

- [[rust-syntax-borrowing]]: lifetimes annotate borrows; understand `&T` vs `&mut T` first.
- [[rust-syntax-ownership]]: lifetimes never apply to owned values.
- [[rust-errors-lifetimes]]: full E0106 / E0623 / E0495 / E0700 / E0759 catalogue with fixes.
- [[rust-syntax-edition-2024]]: RPIT capture default flip, `cargo fix --edition` workflow.
- [[rust-syntax-trait-objects]]: precise capturing in trait definitions (Rust 1.87).
- [[rust-syntax-generics]]: combining `'a` with type parameters.

## References

- `references/methods.md`: full grammar of lifetime annotations on every item kind (fn, struct, enum, trait, impl, where clauses, GATs).
- `references/examples.md`: compilable worked examples of each pattern, including variance demonstrations and HRTB.
- `references/anti-patterns.md`: each anti-pattern with rejected code, error message, and the deterministic fix.

## Sources Verified

- `https://doc.rust-lang.org/reference/lifetime-elision.html` (verified 2026-05-19)
- `https://doc.rust-lang.org/reference/subtyping.html` (verified 2026-05-19)
- `https://doc.rust-lang.org/edition-guide/rust-2024/rpit-lifetime-capture.html` (verified 2026-05-19)
- `https://doc.rust-lang.org/nomicon/subtyping.html` (verified 2026-05-19)
- `https://blog.rust-lang.org/2024/10/17/Rust-1.82.0.html` (precise capturing stabilization, verified 2026-05-19)
