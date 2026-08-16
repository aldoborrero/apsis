# Iterators and closures: working examples

Every example targets Rust 1.85+, edition 2024. Sources cross-referenced to the Rust Book and stdlib docs.

---

## 1. Basic iterator chain

```rust
fn squared_evens(input: &[i32]) -> Vec<i32> {
    input.iter()
        .filter(|&&x| x % 2 == 0)
        .map(|&x| x * x)
        .collect()
}

fn main() {
    let v = vec![1, 2, 3, 4, 5];
    assert_eq!(squared_evens(&v), vec![4, 16]);
}
```

Notes:

- `.iter()` yields `&i32`; the `filter` predicate receives `&&i32`, hence `|&&x|` to bind `x: i32`
- `.map(|&x| x * x)` dereferences the `&i32` once and produces `i32`
- `.collect()` is type-driven by the return type `Vec<i32>`

---

## 2. `collect` with turbofish and `_` inference

```rust
fn main() {
    let nums: Vec<_> = (1..=5).collect();
    assert_eq!(nums, vec![1, 2, 3, 4, 5]);

    let s = (1..=3).map(|n| n.to_string()).collect::<Vec<String>>();
    assert_eq!(s, vec!["1", "2", "3"]);

    // Iterator of (K, V) -> HashMap
    use std::collections::HashMap;
    let map: HashMap<&str, i32> = [("one", 1), ("two", 2)].into_iter().collect();
    assert_eq!(map.get("one"), Some(&1));
}
```

---

## 3. `Result<Vec<T>, E>` short-circuiting collect

```rust
fn parse_all(strs: &[&str]) -> Result<Vec<i32>, std::num::ParseIntError> {
    strs.iter().map(|s| s.parse::<i32>()).collect()
}

fn main() {
    assert_eq!(parse_all(&["1", "2", "3"]).unwrap(), vec![1, 2, 3]);
    assert!(parse_all(&["1", "x", "3"]).is_err());
}
```

The standard library implements `FromIterator<Result<T, E>> for Result<Vec<T>, E>`. The first `Err` short-circuits.

Same trick works for `Option<Vec<T>>`.

---

## 4. `fold` vs `reduce` vs `sum`

```rust
fn main() {
    let sum_fold: i32 = (1..=5).fold(0, |acc, x| acc + x);
    let sum_reduce: i32 = (1..=5).reduce(|acc, x| acc + x).unwrap_or(0);
    let sum_sum: i32 = (1..=5).sum();
    assert_eq!(sum_fold, 15);
    assert_eq!(sum_reduce, 15);
    assert_eq!(sum_sum, 15);
}
```

Decision rule:

- `sum` / `product` for plain numeric reductions
- `reduce` when the accumulator type equals the item type and you want the identity to be the first item
- `fold` when the accumulator type differs from the item type, or when explicit identity is clearer

---

## 5. Custom `Iterator` for a finite counter

```rust
struct Counter {
    count: u32,
    max: u32,
}

impl Counter {
    fn new(max: u32) -> Self { Counter { count: 0, max } }
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

impl ExactSizeIterator for Counter {}

fn main() {
    let c = Counter::new(5);
    let v: Vec<u32> = c.collect();
    assert_eq!(v, vec![1, 2, 3, 4, 5]);
}
```

Implementing `size_hint` allows `collect::<Vec<_>>()` to pre-allocate. Implementing `ExactSizeIterator` (no method needed when `size_hint` returns equal bounds) enables callers to call `.len()`.

---

## 6. `IntoIterator` vs `iter()` vs `into_iter()`

```rust
fn main() {
    let v = vec!["a".to_string(), "b".to_string()];

    // Borrow; v remains usable.
    for s in &v { println!("{s}"); }
    println!("{v:?}"); // OK

    // Mutable borrow.
    let mut v2 = v.clone();
    for s in &mut v2 { s.push('!'); }
    assert_eq!(v2, vec!["a!", "b!"]);

    // Consume.
    let v3 = v.clone();
    for s in v3 {
        // s: String (owned)
        drop(s);
    }
    // v3 no longer accessible here.
}
```

`for s in &v` is sugar for `for s in v.iter()`. `for s in v` is sugar for `for s in v.into_iter()`.

---

## 7. `Fn` closure (immutable captures)

```rust
fn apply<F: Fn(i32) -> i32>(f: F, x: i32) -> i32 { f(x) }

fn main() {
    let factor = 3;
    let multiply = |n| n * factor; // captures `factor` by shared reference
    assert_eq!(apply(&multiply, 5), 15);
    assert_eq!(apply(&multiply, 7), 21); // callable multiple times
    println!("{factor}");                // factor still usable
}
```

---

## 8. `FnMut` closure (mutating captures)

```rust
fn for_each_mut<F: FnMut(i32)>(items: &[i32], mut f: F) {
    for &x in items { f(x); }
}

fn main() {
    let mut sum = 0;
    for_each_mut(&[1, 2, 3], |x| sum += x);
    assert_eq!(sum, 6);
}
```

The compiler infers `FnMut` because the body mutates `sum`.

---

## 9. `FnOnce` closure (consuming captures)

```rust
fn run_once<F: FnOnce() -> String>(f: F) -> String { f() }

fn main() {
    let owned = String::from("hello");
    let consume = move || owned; // body moves `owned` out by value
    let s = run_once(consume);
    assert_eq!(s, "hello");
}
```

The closure body moves `owned` out; it can therefore only be called once.

---

## 10. `move` with `std::thread::spawn`

```rust
use std::thread;

fn main() {
    let data = vec![1, 2, 3];
    let handle = thread::spawn(move || {
        println!("{data:?}");
    });
    handle.join().unwrap();
    // `data` cannot be used here; it was moved into the closure.
}
```

Without `move`, the closure would borrow `data`, but the spawned thread may outlive the borrow. The compiler rejects with E0373 "closure may outlive borrowed value".

---

## 11. `move` with `tokio::spawn`

```rust
// tokio = { version = "1", features = ["full"] }
#[tokio::main]
async fn main() {
    let data = vec![1, 2, 3];
    let handle = tokio::spawn(async move {
        for x in data { println!("{x}"); }
    });
    handle.await.unwrap();
}
```

`async move {}` is the async-block analogue: the future captures `data` by value.

---

## 12. Disjoint capture (edition 2021+)

```rust
struct Point { x: i32, y: i32 }

fn main() {
    let p = Point { x: 1, y: 2 };
    let print_x = || println!("{}", p.x); // captures only `p.x`
    let moved_y = p.y;                    // OK: only `p.y` was free
    print_x();
    println!("{moved_y}");
}
```

Pre-edition-2021 the closure would capture all of `p`, making `let moved_y = p.y;` an error.

---

## 13. Returning `impl Fn` (static dispatch)

```rust
fn make_adder(n: i32) -> impl Fn(i32) -> i32 {
    move |x| x + n
}

fn main() {
    let add5 = make_adder(5);
    assert_eq!(add5(10), 15);
    assert_eq!(add5(20), 25);
}
```

`move` is required: without it the closure would borrow `n` from the stack frame that no longer exists after `make_adder` returns.

---

## 14. Returning `Box<dyn Fn>` (dynamic dispatch)

```rust
fn make_op(kind: &str) -> Box<dyn Fn(i32, i32) -> i32> {
    match kind {
        "add" => Box::new(|a, b| a + b),
        "mul" => Box::new(|a, b| a * b),
        _     => Box::new(|a, _| a),
    }
}

fn main() {
    let op = make_op("add");
    assert_eq!(op(2, 3), 5);
}
```

`Box<dyn Fn>` is required because the three branches return different closure types; `impl Fn` cannot represent that.

---

## 15. Async closure (Rust 1.85)

```rust
// Rust 1.85+, edition 2024
async fn run<F, Fut>(f: F) -> i32
where
    F: AsyncFnOnce(i32) -> i32,
{
    f.async_call_once((10,)).await
}

#[tokio::main]
async fn main() {
    let inc = async |x: i32| x + 1;
    let result = run(inc).await;
    assert_eq!(result, 11);
}
```

For most user code, the sugar suffices:

```rust
let f = async |x: i32| { tokio::time::sleep(std::time::Duration::from_millis(1)).await; x + 1 };
let fut = f(5);
let result = fut.await;
```

---

## 16. `enumerate` for indexed loops

```rust
fn main() {
    for (i, name) in ["alice", "bob", "carol"].iter().enumerate() {
        println!("{i}: {name}");
    }
}
```

Prefer `enumerate` over manually tracking a counter.

---

## 17. `zip` for parallel iteration

```rust
fn main() {
    let names = ["alice", "bob", "carol"];
    let scores = [90, 85, 70];
    for (name, score) in names.iter().zip(scores.iter()) {
        println!("{name}: {score}");
    }
}
```

`zip` stops at the shorter iterator. To preserve "longer + None tail" use `itertools::zip_longest` (out of scope here).

---

## 18. `flat_map` for nested structures

```rust
fn main() {
    let words = vec!["foo bar", "baz qux"];
    let tokens: Vec<&str> = words.iter().flat_map(|s| s.split_whitespace()).collect();
    assert_eq!(tokens, vec!["foo", "bar", "baz", "qux"]);
}
```

`.flat_map(f)` is equivalent to `.map(f).flatten()`.

---

## 19. `inspect` for debug-during-chain

```rust
fn main() {
    let sum: i32 = (1..=5)
        .inspect(|x| println!("input: {x}"))
        .filter(|x| x % 2 == 0)
        .inspect(|x| println!("kept: {x}"))
        .sum();
    println!("total: {sum}");
}
```

`inspect` passes items through unchanged after running the side-effect closure. Removable for production.

---

## 20. Iterator combinators on `Option` (a single-item iterator)

```rust
fn main() {
    let maybe = Some(10);
    let doubled = maybe.iter().map(|&x| x * 2).next();
    assert_eq!(doubled, Some(20));
}
```

`Option<T>` implements `IntoIterator` (yields zero or one item). Prefer `opt.map(f)` for one-step transforms; reach for iterator chains only when the chain has multiple stages.
