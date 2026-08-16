# Borrow-checker anti-patterns

Each entry: the broken-thinking pattern, the code that "fixes" the error wrongly, why it compiles, why it harms, and the correct alternative. These are the traps to NEVER fall into when the borrow checker complains.

---

## AP-1: cloning everything to silence E0382

### The wrong fix

```rust
fn process(data: Vec<u8>) -> usize { data.len() }

fn main() {
    let buffer = vec![0u8; 1_000_000];
    let n = process(buffer.clone()); // ".clone() makes the error go away"
    println!("{} {n}", buffer.len());
}
```

### Why it compiles

`.clone()` produces an independent owned `Vec`, so `buffer` is never moved. The error disappears.

### Why it harms

- It copies one megabyte of heap memory on every call for no reason.
- It hides the real design question: does `process` need to *own* the data, or just read it?
- In a loop or hot path this turns an O(1) borrow into a per-call O(n) allocation.
- `clippy::redundant_clone` flags the obvious cases, but many slip through.

### Correct alternative

Take a borrow. `process` only reads, so it should accept `&[u8]`:

```rust
fn process(data: &[u8]) -> usize { data.len() }

fn main() {
    let buffer = vec![0u8; 1_000_000];
    let n = process(&buffer);        // no copy, no move
    println!("{} {n}", buffer.len());
}
```

Use `.clone()` only when two parts of the program genuinely need independent owned copies. ALWAYS ask "can I borrow instead" first.

---

## AP-2: reaching for RefCell to evade E0502 / E0499

### The wrong fix

```rust
use std::cell::RefCell;

fn main() {
    let data = RefCell::new(vec![1, 2, 3]);
    let first = data.borrow();        // shared runtime borrow
    data.borrow_mut().push(4);        // PANIC at runtime: already borrowed
    println!("{:?}", first);
}
```

### Why it compiles

`RefCell` moves the borrow check from compile time to run time. The compiler no longer sees the conflict, so the code builds.

### Why it harms

- The exact same conflict that E0502 caught at compile time now becomes a runtime `BorrowMutError` panic. You traded a guaranteed-caught bug for a maybe-caught-in-production bug.
- It adds a runtime borrow-flag check on every access.
- It signals "I evaded the checker" instead of "I solved the aliasing problem".

### Correct alternative

Solve the scope conflict the compiler pointed at, exactly as you would for E0502: tighten the shared borrow with NLL, or read into an owned local first.

```rust
fn main() {
    let mut data = vec![1, 2, 3];
    let first = data[0];   // Copy out; shared borrow ends immediately
    data.push(4);          // OK
    println!("{first}");
}
```

`RefCell` is the right tool for genuine interior mutability (graph nodes, observer patterns where the API is fixed to `&self`), NOT for scope errors a refactor solves.

---

## AP-3: promoting to Arc to fix E0596 in single-threaded code

### The wrong fix

```rust
use std::sync::{Arc, Mutex};

struct Counter { n: Arc<Mutex<u32>> }

impl Counter {
    fn bump(&self) {
        *self.n.lock().unwrap() += 1; // "&self stays, problem solved"
    }
}
```

### Why it compiles

`Arc<Mutex<u32>>` provides thread-safe interior mutability, so `bump` can keep `&self` and still mutate.

### Why it harms

- `Mutex::lock` performs an atomic compare-and-swap and can poison on panic; you pay that cost on every increment for a value that never crosses a thread.
- `Arc` adds atomic refcount traffic for no shared-ownership need.
- It is over-engineering: a heavyweight concurrency tool used to dodge a one-keyword fix.

### Correct alternative

If nothing forces `&self`, just declare `&mut self`:

```rust
struct Counter { n: u32 }
impl Counter {
    fn bump(&mut self) { self.n += 1; }
}
```

If a trait fixes the signature to `&self`, use `Cell` (single-threaded, zero-cost):

```rust
use std::cell::Cell;
struct Counter { n: Cell<u32> }
impl Counter {
    fn bump(&self) { self.n.set(self.n.get() + 1); }
}
```

Reach for `Arc<Mutex<T>>` ONLY when the data really is shared across threads.

---

## AP-4: Box::leak to keep a temporary alive for E0716

### The wrong fix

```rust
fn make() -> String { String::from("data") }

fn borrow_it(s: &str) -> &str { s }

fn main() {
    let s: &'static str = Box::leak(make().into_boxed_str()); // "now it's 'static"
    let c = borrow_it(s);
    println!("{c}");
}
```

### Why it compiles

`Box::leak` returns a `&'static mut T` by intentionally never freeing the allocation. A `'static` reference outlives any borrow, so E0716 cannot fire.

### Why it harms

- The allocation is leaked permanently. Every call leaks again. In a long-running service this is an unbounded memory leak.
- It converts a trivial scoping fix into a resource bug that monitoring will eventually flag.

### Correct alternative

Bind the temporary to a `let` so it lives as long as the borrow needs:

```rust
fn main() {
    let s = make();           // owned local, dropped at end of block
    let c = borrow_it(&s);
    println!("{c}");
}
```

`Box::leak` is legitimate only for genuinely process-lifetime data (a config parsed once at startup), never as an E0716 workaround.

---

## AP-5: unsafe and raw pointers to bypass the borrow checker

### The wrong fix

```rust
fn main() {
    let mut v = vec![1, 2, 3];
    let p: *mut i32 = &mut v[0];
    let q: *mut i32 = &mut v[0]; // borrow checker never sees this
    unsafe {
        *p += 1;
        *q += 1; // two mutable aliases: undefined behavior
    }
}
```

### Why it compiles

Raw pointers (`*mut T`) are not tracked by the borrow checker. Creating two of them and writing through both inside `unsafe` silences every E04xx / E05xx error.

### Why it harms

- It creates two mutable aliases to the same memory, which is **undefined behavior** in Rust. The optimiser is allowed to assume `&mut` is unique; violating that can miscompile silently.
- It removes the compiler's soundness guarantee. The borrow checker existed precisely to prevent this; you have switched it off.
- `unsafe` makes *you* responsible for proving no aliasing, and here the proof fails.

### Correct alternative

Use the safe disjoint-borrow API the language provides:

```rust
fn main() {
    let mut v = vec![1, 2, 3];
    let (a, rest) = v.split_first_mut().unwrap();
    *a += 1;
    // operate on `rest` for the other element
    println!("{v:?}");
}
```

`unsafe` is for FFI and verified low-level invariants, NEVER as an escape hatch from a borrow error a refactor solves.

---

## AP-6: holding a lock guard longer to mask E0499

### The wrong fix

```rust
use std::sync::Mutex;

fn main() {
    let m = Mutex::new(0);
    let guard = m.lock().unwrap(); // guard held for the whole function
    // ... lots of unrelated work, no use of the guarded data ...
    drop(guard);
}
```

Variant: keeping a single long-lived `&mut` binding alive across a whole function so a second `&mut` "obviously" cannot exist, instead of scoping each borrow to where it is used.

### Why it compiles

If only one borrow / one guard exists, E0499 cannot fire. Holding it for the entire scope trivially satisfies the checker.

### Why it harms

- A held `MutexGuard` is a held lock: every other thread that needs the mutex blocks for the entire duration, including the unrelated work. This lengthens the critical section and destroys concurrency.
- For plain `&mut`, an over-long borrow blocks every other access to the value, forcing further awkward restructuring downstream.
- It treats "make the checker quiet" as the goal instead of "borrow exactly as long as needed".

### Correct alternative

Scope each borrow / lock to the smallest region that uses it. NLL then ends it at its last use automatically:

```rust
use std::sync::Mutex;

fn main() {
    let m = Mutex::new(0);
    {
        let mut guard = m.lock().unwrap();
        *guard += 1;
    } // lock released here, before the unrelated work
    // ... unrelated work runs without holding the lock ...
}
```

The fix for E0499 is *shorter, well-scoped* borrows, never one borrow stretched to cover everything.

---

## Summary rule

For every borrow-checker error, the correct fix changes the **ownership shape, scope, or mutability declaration** of the code. Any "fix" whose only effect is to make the compiler stop looking (`RefCell` to dodge a scope error, `unsafe` raw pointers, `Box::leak`, blanket `.clone()`) is an anti-pattern: it converts a compile-time-caught bug into a runtime panic, a memory leak, undefined behavior, or a performance regression. ALWAYS solve the problem the error describes; NEVER just hide the error.
