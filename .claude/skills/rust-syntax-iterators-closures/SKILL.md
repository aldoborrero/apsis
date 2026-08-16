---
name: rust-syntax-iterators-closures
description: >
  Use when the user writes iterator chains (map / filter / fold / collect), implements `Iterator` manually, picks between `Fn` / `FnMut` / `FnOnce`, uses `move` closures, encounters closure capture-rules (edition 2021 disjoint captures), or writes async closures (1.85).
  Prevents using imperative loops where adapters fit, picking `FnOnce` when `Fn` would do, forgetting `move` for spawn-style closures, and confusing `collect` type inference.
  Covers: Iterator trait + Item type, adapters (map / filter / take / skip / fold / reduce / collect / sum / count / enumerate / zip / chain / flat_map), lazy evaluation, `collect::<Vec<_>>()` turbofish, custom Iterator impl, IntoIterator vs Iterator, `Fn` / `FnMut` / `FnOnce`, `move` closures, edition-2021 disjoint captures, async closures (1.85), returning closures (`Box<dyn Fn>` vs `impl Fn`).
  Keywords: Iterator, iter, "for loop", map, filter, fold, collect, "into_iter", "iter_mut", closure, "|x|", Fn, FnMut, FnOnce, "move closure", "async closure", "async ||", "disjoint capture", "edition 2021 capture", "lazy iterator", "turbofish", "::<>", "Box<dyn Fn>", "impl Fn", "return closure", "what does map do", "what is collect".
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# rust-syntax-iterators-closures

The **mechanics** of Rust's iterator and closure systems, treated as one topic because closures are predominantly authored as arguments to iterator adapters and they share the same lifetime and trait-bound mental model. Covers the `Iterator` trait contract, the standard adapter set, lazy evaluation, custom `Iterator` impls, `IntoIterator` vs `Iterator`, the `Fn` / `FnMut` / `FnOnce` trait hierarchy, `move` closures, edition-2021 disjoint captures, async closures (Rust 1.85), and returning closures.

Cross-references: [[rust-syntax-traits]] (`Iterator` IS a trait, closure traits are traits) [[rust-syntax-async-await]] (async closures return `impl Future`) [[rust-syntax-ownership]] (`move` captures, by-value vs by-reference) [[rust-syntax-generics]] (generic bounds `T: Iterator`, `F: Fn(...)`) [[rust-syntax-trait-objects]] (`Box<dyn Fn>` vs `impl Fn`).

---

## When to use this skill

- User writes any iterator chain: `.map(...).filter(...).collect()`
- User asks "why doesn't my loop / iterator do anything?" (lazy evaluation)
- User implements a custom `Iterator` for their own type
- User confuses `.iter()`, `.iter_mut()`, and `.into_iter()`
- User picks between `Fn`, `FnMut`, `FnOnce` for a closure-taking API
- User hits "closure may outlive borrowed value" and needs `move`
- User writes `async || { ... }` (Rust 1.85+) and needs the trait story
- User wants to return a closure from a function
- User asks about `collect::<Vec<_>>()` turbofish syntax

For the `for x in xs` desugaring see also [[rust-syntax-control-flow]]. For combinator chains on `Option` / `Result` (which are NOT iterators by default) see [[rust-syntax-option-result]].

---

## The `Iterator` trait (Reference, verbatim)

```rust
pub trait Iterator {
    type Item;
    fn next(&mut self) -> Option<Self::Item>;
    // ... 70+ provided methods (adapters and consumers)
}
```

[Source: std::iter::Iterator][iter-trait]

Two required parts: an associated type `Item` and a single required method `next`. Everything else (`map`, `filter`, `collect`, `fold`, ...) is a provided method built on `next`.

Contract: `next` returns `Some(item)` while items remain, then `None` permanently. An iterator that returns `None` and then later returns `Some` is **fused incorrectly**; wrap with `.fuse()` if behaviour is unknown.

ALWAYS implement only `next` and (optionally) `size_hint` when writing a custom iterator; let the default adapters do the rest.

NEVER call `.next()` on a moved iterator: like any value, an iterator can be moved, after which it is invalid.

---

## `IntoIterator` vs `Iterator` vs three iter-methods

| Call | Yields | Consumes? | Method on |
|------|--------|-----------|-----------|
| `for x in v` | `T` (owned items) | yes (moves `v`) | desugars to `v.into_iter()` |
| `v.into_iter()` | `T` | yes | `IntoIterator for Vec<T>` |
| `v.iter()` | `&T` | no | inherent on `Vec<T>` (calls `<&Vec<T>>::into_iter`) |
| `v.iter_mut()` | `&mut T` | no, but borrows mut | inherent on `Vec<T>` |
| `(&v).into_iter()` | `&T` | no | `IntoIterator for &Vec<T>` |
| `(&mut v).into_iter()` | `&mut T` | no | `IntoIterator for &mut Vec<T>` |

[Source: std::iter::IntoIterator][into-iter-trait]

The `for` loop desugars to `IntoIterator::into_iter`. `for x in &v` is therefore equivalent to `for x in v.iter()`. The most common bug is `for x in v` where `v` is later used: it has been moved.

ALWAYS write `for x in &v` (or `.iter()`) when the collection must remain usable afterwards.

NEVER do `for x in v.iter().collect::<Vec<_>>()`: the `collect` allocates a `Vec` that is then iterated; drop the `.collect()` and iterate `v.iter()` directly.

---

## Adapter quick reference (most common)

| Adapter | Signature shape | Eager? | One-line behaviour |
|---------|----------------|--------|---------------------|
| `map(f)` | `F: FnMut(Self::Item) -> B` | lazy | transform each item |
| `filter(p)` | `P: FnMut(&Self::Item) -> bool` | lazy | keep items where `p(&item)` is true |
| `filter_map(f)` | `F: FnMut(Self::Item) -> Option<B>` | lazy | transform + filter in one |
| `take(n)` | `usize` | lazy | first `n` items |
| `skip(n)` | `usize` | lazy | drop first `n` items |
| `take_while(p)` | predicate | lazy | items until `p` becomes false |
| `skip_while(p)` | predicate | lazy | drop until `p` becomes false |
| `enumerate()` | none | lazy | `(0, a), (1, b), (2, c)` |
| `zip(other)` | `IntoIterator` | lazy | pair up; stops at shorter |
| `chain(other)` | `IntoIterator` | lazy | concat two iterators |
| `flat_map(f)` | `F -> IntoIterator` | lazy | `map` then flatten |
| `flatten()` | none | lazy | flatten `Iterator<Item = IntoIterator>` |
| `rev()` | none (needs `DoubleEndedIterator`) | lazy | reverse direction |
| `peekable()` | none | lazy | adds `.peek()` |
| `inspect(f)` | side-effect closure | lazy | run `f(&item)` and pass through |
| `fold(init, f)` | `B, FnMut(B, Item) -> B` | **eager** | reduce to single value |
| `reduce(f)` | `FnMut(Item, Item) -> Item` | **eager** | fold with first item as init |
| `collect()` | `B: FromIterator<Item>` | **eager** | materialise into a collection |
| `sum() / product()` | none | **eager** | numeric reduction |
| `count()` | none | **eager** | consume and count |
| `any(p) / all(p)` | predicate | **eager** | short-circuiting boolean fold |
| `find(p)` | predicate | **eager** | first matching item |
| `for_each(f)` | `FnMut(Item)` | **eager** | side-effect each item |

Full method index and signatures: see `references/methods.md`.

---

## Lazy evaluation: the single most common iterator bug

Adapters do nothing on their own. They build a description of work; only a **consumer** (`collect`, `for`, `sum`, `count`, `fold`, `reduce`, `any`, `all`, `find`, `for_each`, manual `next()`) drives the work.

```rust
// BUG: no side effect happens
v.iter().map(|x| println!("{x}"));

// FIX: collect, count, or for_each forces evaluation
v.iter().for_each(|x| println!("{x}"));
```

Compiler help: the `#[must_use]` attribute on `Iterator` and its adapters triggers `unused_must_use` warnings. ALWAYS heed that warning, do not silence it.

ALWAYS end an iterator chain with a consumer (`collect`, `for`, `sum`, ...). The chain is otherwise dead code.

NEVER assume `.map(|x| do_side_effect(x))` runs without a consumer.

---

## `collect` and the turbofish `::<>`

`collect` is generic over its output type. The compiler usually needs a hint:

```rust
// Option 1: annotate the binding (preferred when feasible)
let v: Vec<i32> = (0..5).collect();

// Option 2: turbofish on collect itself
let v = (0..5).collect::<Vec<i32>>();
let v = (0..5).collect::<Vec<_>>(); // `_` lets the compiler infer the item type

// Option 3: collect into a HashMap from (K, V) pairs
use std::collections::HashMap;
let m: HashMap<&str, i32> = [("a", 1), ("b", 2)].into_iter().collect();
```

[Source: std::iter::Iterator::collect][collect]

Common targets: `Vec<T>`, `String` (from `Iterator<Item = char>` or `Item = &str`), `HashMap<K, V>`, `HashSet<T>`, `BTreeMap<K, V>`, `Result<Vec<T>, E>` (short-circuits on first `Err`), `Option<Vec<T>>` (short-circuits on first `None`).

The `Result<Vec<T>, E>` trick is idiomatic for "parse all or fail":

```rust
let parsed: Result<Vec<i32>, _> = ["1", "2", "3"].iter().map(|s| s.parse()).collect();
```

ALWAYS use `_` placeholders inside turbofish (`Vec<_>`) when the item type is obvious from the chain.

NEVER write `.collect::<Vec<_>>().iter()` to immediately re-iterate; remove both the `collect` and the trailing `.iter()`.

---

## Custom `Iterator` implementation

```rust
struct Counter {
    count: u32,
    max: u32,
}

impl Iterator for Counter {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.count < self.max {
            self.count += 1;
            Some(self.count)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.max - self.count) as usize;
        (remaining, Some(remaining))
    }
}
```

[Source: std::iter::Iterator::size_hint][size-hint]

`size_hint` returns `(lower_bound, Option<upper_bound>)`. Implementing it correctly lets adapters like `collect` pre-allocate. An over-reported lower bound is a soundness bug in `unsafe` consumers; ALWAYS keep `size_hint` conservative (under-report is sound, over-report is not).

Marker traits to opt into when applicable:

- `ExactSizeIterator` (provide `len`) when the count is known and equals `size_hint().0 == size_hint().1.unwrap()`
- `DoubleEndedIterator` (provide `next_back`) when iteration can run from either end
- `FusedIterator` when `next` returning `None` is permanent

ALWAYS implement only `next` plus optional `size_hint` for a basic custom iterator; the 70+ adapters come for free.

NEVER implement `Iterator::count` or `Iterator::last` manually unless you have a more efficient algorithm than the default; the default works in O(n) via `next`.

---

## Closures: syntax and the three traits

A closure expression: `|param_list| expression_or_block`. Type annotations on parameters are optional; the compiler infers them at first call site. Once inferred, a closure has a **single** call signature.

The closure-trait hierarchy ([Source: std::ops::Fn][fn-trait]):

```text
FnOnce  (call at least once, consumes captures)
   ^
   |  supertrait
FnMut   (call multiple times, may mutate captures)
   ^
   |  supertrait
Fn      (call multiple times, immutable captures)
```

A closure implements:

| Closure body uses captures by ... | Implements | Implements | Implements |
|-----------------------------------|------------|------------|------------|
| value (consumes) | `FnOnce` | | |
| `&mut` reference | `FnOnce` | `FnMut` | |
| `&` reference (or `Copy` capture) | `FnOnce` | `FnMut` | `Fn` |

The compiler picks the **most-permissive** trait the body allows. An API accepting `F: Fn(T) -> U` accepts every `Fn` closure (and also `fn` items); an API accepting `F: FnOnce(T) -> U` accepts the broadest set of closures.

ALWAYS bound parameter-taking APIs by the **loosest** trait that suffices (prefer `FnOnce` when you call once, `FnMut` when you call repeatedly and may mutate state, `Fn` when you call repeatedly without mutation).

NEVER bound by `Fn` when `FnMut` suffices: that needlessly rejects callers whose closures mutate captures.

---

## `move` closures

By default a closure borrows its captures. `move` forces capture-by-value:

```rust
let data = vec![1, 2, 3];
std::thread::spawn(move || {
    println!("{data:?}"); // takes ownership of `data`
});
// `data` is no longer accessible here
```

ALWAYS use `move` when the closure outlives the current stack frame: `thread::spawn`, `tokio::spawn`, async tasks stored in a struct, callbacks given to a runtime, return-position closures.

NEVER use `move` reflexively. A closure that does not escape the current scope is cleaner without `move` (the borrow checker still verifies correctness).

`move` does NOT change which trait the closure implements: a `move` closure that only reads `Copy` captures still implements `Fn`. The `move` keyword controls **how** captures are taken, the body controls **which** trait is implemented.

---

## Edition 2021 disjoint captures

Pre-edition-2021 closures captured an entire struct even when the body only used one field. Edition 2021+ captures the individual paths the body uses:

```rust
struct Point { x: i32, y: i32 }
let p = Point { x: 1, y: 2 };

// Edition 2021+: closure captures only `p.x`, leaving `p.y` movable.
let c = || println!("{}", p.x);
let y = p.y; // OK in 2021+, was an error pre-2021
c();
```

[Source: Rust 2021 disjoint capture][ed2021-capture]

Edition 2024 inherits this rule. Skills target edition 2024, so ALWAYS rely on disjoint capture. If targeting an older crate at edition 2018, use `let _ = &p.x;` shadow-binds inside the closure to force the same behaviour, or upgrade edition.

NEVER assume closures capture by struct as a whole. Closure capture in edition 2021+ is per-field-path.

---

## Async closures (Rust 1.85)

```rust
let f = async |x: i32| { x + 1 };
let fut = f(5);            // returns an impl Future<Output = i32>
let result = fut.await;    // 6
```

[Source: Rust 1.85.0 release][rust185]

Three closure-trait variants exist for async:

| Trait | Calls returning future | Captures |
|-------|------------------------|----------|
| `AsyncFn(...) -> ...` | many | immutable |
| `AsyncFnMut(...) -> ...` | many | mutable |
| `AsyncFnOnce(...) -> ...` | one | by value |

The future returned is anonymous. For trait-object use, wrap in `Pin<Box<dyn Future<Output = T>>>` because none of the `AsyncFn` family is dyn-compatible. For static dispatch, use `F: AsyncFn(...) -> ...` bounds.

ALWAYS use `async ||` (Rust 1.85+) when you need a closure that returns a future captured at the call site; older code uses `|| async { ... }`, which is a regular `Fn` returning an `impl Future` (subtly different lifetime story).

NEVER mix `async fn` and `async ||`: an `async fn item` is a function-item type, an `async ||` is a closure. They satisfy different trait bounds.

---

## Returning a closure from a function

Two choices ([Source: Rust Book ch. 13.1][book-closures]):

```rust
// Static dispatch, monomorphised, zero indirection (preferred).
fn make_adder(n: i32) -> impl Fn(i32) -> i32 {
    move |x| x + n
}

// Dynamic dispatch, vtable indirection (use when the return type varies).
fn make_adder_dyn(n: i32) -> Box<dyn Fn(i32) -> i32> {
    Box::new(move |x| x + n)
}
```

Decision rule:

- `-> impl Fn(...)` when a single concrete type is fine and the body is monomorphic
- `-> Box<dyn Fn(...)>` when the function may return one of several closure types (e.g. behind an `if`/`match`)

ALWAYS use `move` in the closure body of a function returning a closure: locals would otherwise be dangling references.

NEVER write `-> impl Fn` from a function that may return closures of different shapes; that is a compile error and requires `Box<dyn Fn>` (or an enum-of-closures pattern).

---

## Decision tree: pick the right closure trait bound

```text
Does the API call the closure?
    no  -> just store it. Use Box<dyn Fn...> or impl Fn... as appropriate.
    once -> bound by FnOnce
    many times -> does the closure need to mutate captured state?
                  yes -> bound by FnMut
                  no  -> bound by Fn
```

For async APIs, replace `Fn` family with `AsyncFn` family (Rust 1.85+).

---

## Anti-patterns (short list, full set in `references/anti-patterns.md`)

- Imperative `for` loop that recomputes an accumulator where `fold` / `sum` / `count` is a one-liner
- `.collect::<Vec<_>>()` materialised only to immediately call `.iter()` on it
- Picking `Fn` when `FnMut` would do: callers with mutating closures are needlessly rejected
- Forgetting `move` in `tokio::spawn` / `thread::spawn` closures: "closure may outlive borrowed value" (E0373)
- `.iter().map(|x| x.clone())` instead of `.cloned()` (clippy::map_clone)
- `.filter(...).count()` to test "any matches" instead of `.any(...)` (loses short-circuiting)
- Iterator chain with no consumer: `unused_must_use` warning silenced
- `match opt { Some(x) => Some(f(x)), None => None }` instead of `opt.map(f)`

Full anti-pattern catalogue with WHY-each-fails: `references/anti-patterns.md`.

---

## Reference files

- `references/methods.md` : full signatures of `Iterator` adapters and `Fn` / `FnMut` / `FnOnce` traits
- `references/examples.md` : working code samples for every adapter + closure pattern in this skill
- `references/anti-patterns.md` : 10+ anti-patterns with root-cause and fix

[iter-trait]: https://doc.rust-lang.org/std/iter/trait.Iterator.html
[into-iter-trait]: https://doc.rust-lang.org/std/iter/trait.IntoIterator.html
[collect]: https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.collect
[size-hint]: https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.size_hint
[fn-trait]: https://doc.rust-lang.org/std/ops/trait.Fn.html
[book-closures]: https://doc.rust-lang.org/book/ch13-01-closures.html
[book-iterators]: https://doc.rust-lang.org/book/ch13-02-iterators.html
[ed2021-capture]: https://doc.rust-lang.org/edition-guide/rust-2021/disjoint-capture-in-closures.html
[rust185]: https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/
