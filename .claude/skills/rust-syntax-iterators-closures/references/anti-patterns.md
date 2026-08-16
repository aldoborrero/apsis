# Anti-patterns: iterators and closures

Each entry: the buggy code, WHY it fails, the fix. Sourced from the Rust Book, stdlib documentation, and `clippy` lint catalogue.

---

## AP-1: Iterator chain with no consumer

```rust
// BUG
let v = vec![1, 2, 3];
v.iter().map(|x| println!("{x}"));
```

WHY it fails: `map` is lazy. With no `collect`, `for_each`, `count`, or other consumer, **no closure call happens**. The compiler emits `unused_must_use` because `Iterator` is `#[must_use]`.

FIX:

```rust
// Option 1: for_each
v.iter().for_each(|x| println!("{x}"));
// Option 2: a regular for loop
for x in &v { println!("{x}"); }
```

NEVER silence `unused_must_use` on an iterator chain. It always indicates the chain is dead code.

---

## AP-2: `collect` then immediately re-iterate

```rust
// BUG
let v = vec![1, 2, 3];
let doubled: Vec<i32> = v.iter().map(|x| x * 2).collect::<Vec<_>>();
let total: i32 = doubled.iter().sum();
```

WHY it fails: the intermediate `Vec` allocates and copies pointlessly; the iterator already represents the doubled values.

FIX:

```rust
let total: i32 = v.iter().map(|x| x * 2).sum();
```

Keep iterators lazy until the final consumer. Materialise to `Vec` only when the values must be stored, iterated multiple times, or indexed.

---

## AP-3: Imperative loop where adapters fit

```rust
// BUG
let mut total = 0;
let mut count = 0;
for &x in &nums {
    if x > 0 {
        total += x;
        count += 1;
    }
}
let avg = total as f64 / count as f64;
```

WHY it fails: hides the intent ("average of positives"), gives the loop body responsibility for two unrelated accumulators, and is harder to refactor.

FIX:

```rust
let positives: Vec<i32> = nums.iter().filter(|&&x| x > 0).copied().collect();
let avg = positives.iter().sum::<i32>() as f64 / positives.len() as f64;
// Or without intermediate Vec:
let (sum, count) = nums.iter().filter(|&&x| x > 0).fold((0i32, 0usize), |(s, c), &x| (s + x, c + 1));
let avg = sum as f64 / count as f64;
```

Choose `filter + sum + count` for readability, or `fold` for a single pass when both totals are needed.

---

## AP-4: Picking `Fn` when `FnMut` would do

```rust
// BUG: needlessly rejects callers that mutate captures
fn run_each<F: Fn(i32)>(items: &[i32], f: F) {
    for &x in items { f(x); }
}

let mut count = 0;
run_each(&[1, 2, 3], |_| count += 1); // ERROR: closure mutates `count`, only FnMut
```

WHY it fails: `Fn` requires the closure's captures to be used immutably; closures with mutable state can never satisfy that bound.

FIX:

```rust
fn run_each<F: FnMut(i32)>(items: &[i32], mut f: F) {
    for &x in items { f(x); }
}
```

Rule: bound by the **loosest** trait you actually need. `FnOnce` < `FnMut` < `Fn` in restrictiveness on the closure body, but `FnOnce` is broadest in **callers it accepts**.

---

## AP-5: Forgetting `move` in spawn closures

```rust
// BUG
fn launch() {
    let data = vec![1, 2, 3];
    std::thread::spawn(|| {
        println!("{data:?}");
    });
} // `data` dropped here, but the thread may still run
```

WHY it fails: the closure borrows `data`, but the thread can outlive the borrow. Compiler error E0373: "closure may outlive the current function".

FIX:

```rust
fn launch() {
    let data = vec![1, 2, 3];
    std::thread::spawn(move || {
        println!("{data:?}");
    });
}
```

Use `move` for any closure that escapes the current stack frame: `thread::spawn`, `tokio::spawn`, returned-from-function closures, closures stored in a struct.

---

## AP-6: `.iter().map(|x| x.clone())` instead of `.cloned()`

```rust
// BUG (clippy::map_clone)
let owned: Vec<String> = strs.iter().map(|s| s.clone()).collect();
```

WHY it fails: less idiomatic, hides intent. `.cloned()` documents the operation in one identifier.

FIX:

```rust
let owned: Vec<String> = strs.iter().cloned().collect();
// or .copied() if items are Copy:
let copied: Vec<i32> = nums.iter().copied().collect();
```

Source: `clippy::map_clone`, https://rust-lang.github.io/rust-clippy/master/#map_clone

---

## AP-7: `.filter(...).count()` to test existence

```rust
// BUG
if items.iter().filter(|x| x.is_active()).count() > 0 { /* ... */ }
```

WHY it fails: counts every match, when one is enough. Loses short-circuiting on large collections.

FIX:

```rust
if items.iter().any(|x| x.is_active()) { /* ... */ }
```

`.any(p)` returns at the first match. Similarly use `.all(p)` for universal checks.

---

## AP-8: `match` ladder instead of `map` / `and_then`

```rust
// BUG
let out = match maybe_n {
    Some(n) => Some(n * 2),
    None => None,
};
```

WHY it fails: noisy and re-encodes what `Option::map` already does. The compiler accepts both, but every reviewer pays cognitive cost.

FIX:

```rust
let out = maybe_n.map(|n| n * 2);
```

For chaining `Option<Option<T>>` or `Option<Result<T, E>>` flattening, use `and_then` and `flatten`.

---

## AP-9: Wrong closure parameter pattern on `filter`

```rust
// BUG
let evens: Vec<&i32> = v.iter().filter(|x| x % 2 == 0).collect();
```

WHY it fails: `filter` receives `&Self::Item`. For `v.iter()` the item is `&i32`, so the predicate sees `&&i32`. Auto-deref makes the body compile, but the parameter pattern is unclear.

FIX:

```rust
let evens: Vec<&i32> = v.iter().filter(|&&x| x % 2 == 0).collect();
// or destructure explicitly:
let evens: Vec<&i32> = v.iter().filter(|n| **n % 2 == 0).collect();
```

ALWAYS write `|&&x|` (or `|&x|` after `into_iter` on `Vec<i32>`) when filtering primitives to keep the type in view.

---

## AP-10: Over-collecting before passing to a function

```rust
// BUG
fn print_all(items: Vec<i32>) {
    for x in items { println!("{x}"); }
}

let v: Vec<i32> = (0..10).collect();
print_all(v.iter().copied().collect()); // allocates twice
```

WHY it fails: `print_all` accepts a concrete `Vec<i32>`, so the caller is forced to collect. The API should accept `IntoIterator` to keep iteration lazy.

FIX:

```rust
fn print_all<I: IntoIterator<Item = i32>>(items: I) {
    for x in items { println!("{x}"); }
}

print_all(0..10);                          // no Vec at all
print_all(v.iter().copied());              // no extra collect
print_all(v);                              // also fine: Vec is IntoIterator
```

ALWAYS write iterator-consuming APIs as `IntoIterator` to maximise caller flexibility.

---

## AP-11: Confusing `into_iter()` semantics across containers

```rust
// BUG (Rust 2021): old slice behaviour was different in earlier editions
fn sum(slice: &[i32]) -> i32 {
    slice.into_iter().sum() // edition 2021+: yields &i32, OK
}
```

WHY it fails: pre-2021 editions, `[T; N]::into_iter()` yielded `&T`, not `T`, leading to surprises. Edition 2021 changed array `into_iter` to consume.

FIX (edition 2024 baseline): edition 2024 inherits 2021 semantics. ALWAYS check edition in `Cargo.toml`: `edition = "2024"`. For arrays specifically, `arr.into_iter()` yields owned items; for `&arr.into_iter()` it yields refs.

```rust
let arr = [1, 2, 3];
let owned: Vec<i32> = arr.into_iter().collect();      // edition 2021+
let borrowed: Vec<&i32> = (&arr).into_iter().collect();
```

---

## AP-12: Returning closures with inconsistent types via `impl Fn`

```rust
// BUG
fn op(kind: bool) -> impl Fn(i32) -> i32 {
    if kind {
        |x| x + 1
    } else {
        |x| x * 2 // ERROR: different closure type
    }
}
```

WHY it fails: `impl Fn` returns a **single** concrete type. The two branches produce different anonymous closure types; the compiler cannot unify them.

FIX:

```rust
fn op(kind: bool) -> Box<dyn Fn(i32) -> i32> {
    if kind {
        Box::new(|x| x + 1)
    } else {
        Box::new(|x| x * 2)
    }
}
```

Use `impl Fn` only when the function has exactly one closure-return path. For branching, use `Box<dyn Fn>` (heap, vtable) or refactor to an enum-of-closures pattern.

---

## AP-13: Capturing `&mut` references in async closures

```rust
// BUG: roughly approximates a common compile error
async fn run(mut counter: i32) {
    let inc = async || { counter += 1; }; // AsyncFnMut
    inc.await;
    inc.await; // OK
    println!("{counter}");
}
```

This compiles; the danger is calling such a closure concurrently. Async closures with `&mut` captures are `AsyncFnMut`, which forbid concurrent calls. Trying to drive two futures from the same `AsyncFnMut` simultaneously is rejected.

FIX:

- Use `Arc<Mutex<_>>` or atomic primitives for shared mutable state across concurrent futures.
- Or keep state local and pass updates back via channels.

---

## AP-14: `flatten` on the wrong shape

```rust
// BUG
let v: Vec<Result<i32, String>> = vec![Ok(1), Err("x".into()), Ok(3)];
let flat: Vec<i32> = v.into_iter().flatten().collect(); // ERROR or surprising
```

WHY it fails: `Result<T, E>` implements `IntoIterator` (yields `Ok(t)` as one item, `Err(_)` as zero items). The error is silently dropped, which is rarely intended.

FIX: be explicit about what should happen on `Err`:

```rust
// Short-circuit on first Err.
let collected: Result<Vec<i32>, String> = v.into_iter().collect();
// Or partition into oks/errs.
let (oks, errs): (Vec<_>, Vec<_>) = v.into_iter().partition(Result::is_ok);
```

---

## Compiler error index references

- E0373: "closure may outlive the current function" - missing `move`
- E0507: "cannot move out of borrowed content" - body moves a borrowed capture
- E0596: "cannot borrow ... as mutable, as it is not declared as mutable" - `FnMut` body needs the binding to be `mut`
- E0525: "expected a closure that implements the Fn trait, but this closure only implements FnMut" - bound too strict for body

Full index: https://doc.rust-lang.org/error_codes/error-index.html
