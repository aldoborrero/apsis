# Borrow-checker fix recipes: API reference

Complete signatures and rules for every fix family in `rust-errors-borrow-checker`. All targets Rust 1.85+, edition 2024.

---

## 1. The borrow rules (verbatim)

From The Rust Book, Ch. 4.2:

> At any given time, you can have **either one mutable reference or any number of immutable references**. References must always be valid.

Encoded as a truth table on a single value `x`:

| Active references on `x` | Allowed | Error code on violation |
|---|---|---|
| zero | yes | none |
| N x `&x` (N >= 1), no `&mut x` | yes | none |
| exactly one `&mut x`, no `&x` | yes | none |
| any `&x` + any `&mut x` overlapping | NO | E0502 |
| two or more `&mut x` overlapping | NO | E0499 |
| `&mut x` where `x` not declared `mut` | NO | E0596 |
| `&mut` to a temporary outliving its statement | NO | E0716 |
| value used after a move | NO | E0382 |
| closure unique-borrows a value already borrowed | NO | E0500 |

---

## 2. Non-Lexical Lifetimes (NLL)

NLL has been stable since Rust 2018 and is the default model in every edition since. Key rule:

> A borrow ends at its **last use**, not at the end of the enclosing lexical scope.

Consequences for fixes:

- A `&` or `&mut` that is never used again is **dead** at that point; a conflicting borrow afterwards is legal.
- Moving the second access *after* the last use of the first borrow resolves E0502 and E0499 without any clone.
- A borrow whose last use is inside a branch ends at the end of that branch.

```rust
let mut v = vec![1, 2, 3];
let first = &v[0];      // borrow starts
println!("{first}");    // last use of `first` -> borrow ends HERE
v.push(4);              // OK: no live borrow
```

NLL does not relax the rules; it only narrows *when* a borrow is considered live. It cannot fix a genuinely overlapping borrow.

---

## 3. Two-phase borrows

Stable since Rust 1.18. A `&mut` argument is split into:

1. **Reservation phase**: acts like a shared `&` borrow. Other shared borrows are allowed.
2. **Activation phase**: becomes a true `&mut` at the point of use.

This is what makes `vec.push(vec.len())` compile: `vec.len()` runs during the reservation phase. If your code looks like this and still fails, the conflict is elsewhere (usually a separately-named binding holding a `&`), and it is a real E0499 / E0502.

---

## 4. `std::mem::take`

Signature:

```rust
pub fn take<T: Default>(dest: &mut T) -> T
```

- Replaces `*dest` with `T::default()` and returns the previous value.
- Requires `T: Default`.
- Use to extract an owned value out of a `&mut` location without violating E0382, leaving the location validly initialised.
- Source: https://doc.rust-lang.org/std/mem/fn.take.html

```rust
use std::mem;
struct Buf { data: Vec<u8> }
impl Buf {
    fn drain(&mut self) -> Vec<u8> {
        mem::take(&mut self.data) // self.data becomes empty Vec
    }
}
```

---

## 5. `std::mem::replace`

Signature:

```rust
pub fn replace<T>(dest: &mut T, src: T) -> T
```

- Moves `src` into `*dest` and returns the previous `*dest`.
- No `Default` bound; you supply the replacement explicitly.
- Use when the replacement is not `T::default()`.
- Source: https://doc.rust-lang.org/std/mem/fn.replace.html

```rust
use std::mem;
enum State { Idle, Running(String) }
fn stop(s: &mut State) -> State {
    mem::replace(s, State::Idle)
}
```

## 6. `std::mem::swap`

Signature:

```rust
pub fn swap<T>(x: &mut T, y: &mut T)
```

- Exchanges the values behind two mutable references without moving either out.
- Use to reorder ownership between two `&mut` locations.
- Source: https://doc.rust-lang.org/std/mem/fn.swap.html

---

## 7. Split borrows on struct fields

The borrow checker tracks **disjoint paths**. Borrowing two distinct fields of the same struct mutably is legal:

```rust
struct Pair { left: Vec<i32>, right: Vec<i32> }
let mut p = Pair { left: vec![], right: vec![] };
let l = &mut p.left;
let r = &mut p.right;  // OK: disjoint field paths
l.push(1);
r.push(2);
```

Limitations:

- Works only when the compiler can statically prove the paths are disjoint (literal field access).
- Does NOT work through a method call: `p.left_mut()` and `p.right_mut()` each borrow the *whole* `&mut self`, so calling both yields E0499. Fix: a single method returning a tuple of `&mut` to both fields, or access fields directly.

```rust
impl Pair {
    fn both_mut(&mut self) -> (&mut Vec<i32>, &mut Vec<i32>) {
        (&mut self.left, &mut self.right) // compiler sees disjoint paths
    }
}
```

---

## 8. Split borrows on slices

`slice::split_at_mut`:

```rust
pub fn split_at_mut(&mut self, mid: usize) -> (&mut [T], &mut [T])
```

- Returns two non-overlapping mutable subslices: `[0, mid)` and `[mid, len)`.
- Panics if `mid > len`.
- The canonical safe way to get two `&mut` into one `Vec` / array / slice.
- Source: https://doc.rust-lang.org/std/primitive.slice.html#method.split_at_mut

Related: `split_first_mut`, `split_last_mut`, `chunks_mut`, `iter_mut` (one `&mut T` at a time), `get_disjoint_mut` (stable 1.86, multiple disjoint indices at once).

```rust
let mut v = [1, 2, 3, 4, 5];
let (a, b) = v.split_at_mut(2);
a[0] = 10;
b[0] = 30;
```

---

## 9. Interior mutability primitives

When a method must keep `&self` (trait signature, shared API) but mutate state, use interior mutability instead of forcing `&mut self` (E0596 fix pattern 3).

### `Cell<T>`

```rust
impl<T> Cell<T> {
    pub fn new(value: T) -> Cell<T>;
    pub fn set(&self, val: T);
    pub fn get(&self) -> T where T: Copy;
    pub fn replace(&self, val: T) -> T;
    pub fn take(&self) -> T where T: Default;
}
```

- No runtime borrow tracking; you can never get a reference to the inner value, only move it in or out.
- Zero runtime cost. `!Sync` (single-threaded only).
- Best for `Copy` types (counters, flags).
- Source: https://doc.rust-lang.org/std/cell/struct.Cell.html

### `RefCell<T>`

```rust
impl<T> RefCell<T> {
    pub fn new(value: T) -> RefCell<T>;
    pub fn borrow(&self) -> Ref<'_, T>;        // panics if a &mut is active
    pub fn borrow_mut(&self) -> RefMut<'_, T>;  // panics if any borrow is active
    pub fn try_borrow(&self) -> Result<Ref<'_, T>, BorrowError>;
    pub fn try_borrow_mut(&self) -> Result<RefMut<'_, T>, BorrowMutError>;
}
```

- Enforces the borrow rules at **runtime**. Violation panics (`BorrowMutError`).
- `!Sync` (single-threaded only).
- Use only when the compile-time check genuinely cannot express the pattern (graph nodes, observer registries). It moves a compile error to a runtime panic, so prefer a refactor first.
- Source: https://doc.rust-lang.org/std/cell/struct.RefCell.html

### `Mutex<T>` / `RwLock<T>`

Thread-safe interior mutability. Use these only when the data really crosses threads. For single-threaded E0596, `Cell` / `RefCell` is correct; `Mutex` adds atomic and poisoning overhead for no benefit.

- `std::sync::Mutex`: https://doc.rust-lang.org/std/sync/struct.Mutex.html
- `std::sync::RwLock`: https://doc.rust-lang.org/std/sync/struct.RwLock.html

---

## 10. Receiver conversion table for E0596

| Current receiver | Mutation needed | Fix |
|---|---|---|
| `&self` | mutate a plain field | change to `&mut self` |
| `&self`, signature fixed by trait | mutate a `Copy` field | wrap field in `Cell<T>` |
| `&self`, signature fixed by trait | mutate a non-`Copy` field | wrap field in `RefCell<T>` |
| `&self`, shared across threads | mutate any field | wrap field in `Mutex<T>` / `RwLock<T>` |
| `self` (by value) | any | already exclusive; no error |
| local `let x` | `&mut x` needed | change to `let mut x` |

---

## 11. Temporary lifetime extension rules (E0716)

From The Rust Reference, "Temporary lifetimes":

- A temporary created by an rvalue normally drops at the **end of the enclosing statement**.
- **Lifetime extension** applies when the temporary is the operand of a `&` or `&mut` in a `let` initialiser: the temporary is promoted to the lifetime of the `let` binding's block.

```rust
let p = &foo();      // temporary EXTENDED to block scope (extension applies)
let p = bar(&foo()); // temporary NOT extended (the & is buried in a call arg) -> E0716
```

Reliable fix: bind the value to its own `let` first.

```rust
let value = foo();   // owns the value for the whole block
let p = bar(&value); // borrows a real local; no temporary
```

- Source: https://doc.rust-lang.org/reference/destructors.html#temporary-lifetime-extension

---

## 12. `Rc` / `Arc` (shared ownership, distinct from interior mutability)

`Rc<T>` (single-thread) and `Arc<T>` (thread-safe) give **multiple owners** of the same value. They are the correct fix when E0382 arises because two parts of a program genuinely need to co-own one value (graph, cache). They are NOT a fix for E0502 / E0596 / E0499 scope errors. `Rc<T>` / `Arc<T>` alone give shared *immutable* access; combine with `RefCell` / `Mutex` for shared mutable access.

- `std::rc::Rc`: https://doc.rust-lang.org/std/rc/struct.Rc.html
- `std::sync::Arc`: https://doc.rust-lang.org/std/sync/struct.Arc.html
