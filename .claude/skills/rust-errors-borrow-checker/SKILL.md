---
name: rust-errors-borrow-checker
description: >
  Use when the user hits borrow-checker error codes E0382 / E0502 / E0596 / E0499 / E0500 / E0716, asks "why can't I use this after move", "why can't I have & and &mut at the same time", or needs a recipe to satisfy the borrow checker.
  Prevents adding `.clone()` everywhere as the universal fix, missing NLL improvements, and using RefCell to evade the rules when a refactor suffices.
  Covers: E0382 use of moved value, E0502 mutable+immutable borrow at same time, E0596 cannot borrow as mutable, E0500 closure borrow conflict, E0499 multiple mutable borrows, E0716 temporary value dropped while borrowed; fix patterns (clone, restructure, take(), mem::replace, split borrow, introduce intermediate binding, scope tightening via NLL).
  Keywords: E0382, E0502, E0596, E0499, E0500, E0716, "borrow checker", "moved value", "use of moved value", "cannot borrow as mutable", "cannot borrow as immutable while", "already mutably borrowed", "temporary value dropped", "doesn't live long enough", "borrow checker error", "fix borrow", "Rust won't compile", "ownership error".
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# rust-errors-borrow-checker

How to read and fix the six most common borrow-checker errors: E0382, E0502, E0596, E0499, E0500, E0716. Every fix patches the **root cause** (ownership shape, scope, mutability declaration), not the symptom. `.clone()` is the **last resort**, not the first.

Cross-references: [[rust-syntax-ownership]] (move semantics), [[rust-syntax-borrowing]] (`&T` vs `&mut T` rules, NLL, split borrows), [[rust-syntax-lifetimes]] (lifetime annotations on borrows), [[rust-errors-lifetimes]] (E0106 / E0623 / E0495 / E0700 / E0759), [[rust-agents-compile-fix]] (orchestration recipe for any rustc error).

---

## When to use this skill

- Compiler reports one of E0382, E0502, E0596, E0499, E0500, E0716 (or the post-NLL un-numbered variants that print the same message).
- User asks "why can't I use this variable after I passed it / assigned it / matched on it".
- User asks "why does Rust forbid `&` and `&mut` together".
- User pastes a borrow-checker message and asks for a fix.
- User reaches for `.clone()` as the default escape hatch.

If the message contains "lifetime" or `'a` / `'static` symbols, use [[rust-errors-lifetimes]] instead.

---

## Decision tree: which error, which fix family

```
+--- rustc error message ---+
|                           |
|  "use of moved value"     | -> E0382 -> see "E0382" section. Fix family: avoid the move, borrow instead, or clone if data is small.
|                           |
|  "cannot borrow ... as    | -> E0502 -> see "E0502" section. Fix family: tighten the shared-borrow scope (NLL).
|   mutable because it is   |
|   also borrowed as        |
|   immutable"              |
|                           |
|  "cannot borrow ... as    | -> E0596 -> see "E0596" section. Fix family: declare `mut`, change receiver to `&mut self`.
|   mutable, as it is not   |
|   declared as mutable"    |
|                           |
|  "cannot borrow ... as    | -> E0499 -> see "E0499" section. Fix family: scope-tighten or split borrow across disjoint paths.
|   mutable more than once  |
|   at a time"              |
|                           |
|  "closure requires unique | -> E0500 -> see "E0500" section. Fix family: drop the conflicting borrow before the closure or restructure capture.
|   access to ... but it    |
|   is already borrowed"    |
|                           |
|  "temporary value dropped | -> E0716 -> see "E0716" section. Fix family: `let`-bind the temporary so it outlives the borrow.
|   while borrowed"         |
|                           |
+---------------------------+
```

ALWAYS map the message to the error code first. NEVER guess a fix from the line number alone.

---

## E0382: use of moved value

**Official summary (verbatim):** *"A variable was used after its contents have been moved elsewhere."* Source: `rustc_error_codes/E0382.md`.

### Root cause

A non-`Copy` value (`String`, `Vec`, `Box`, custom struct without `derive(Copy)`, etc.) was moved by:
- assignment to another binding (`let y = x;`)
- pass to a function by value (`takes_ownership(x);`)
- pattern bind in `match` / `if let` without `ref` / `ref mut`
- being captured by-move in a closure
- being put into a container (`vec.push(x);`)

After the move, the original binding is uninitialised. Using it triggers E0382.

### Erroneous example

```rust
struct MyStruct { s: u32 }

fn main() {
    let mut x = MyStruct { s: 5 };
    let y = x;          // moves x into y
    x.s = 6;            // E0382: use of moved value `x`
    println!("{}", x.s);
}
```

### Fix patterns (apply in this priority order)

1. **Borrow instead of move.** If the consumer only reads, take `&T`. If it writes, take `&mut T`. ALWAYS try this first.
   ```rust
   fn calculate_length(s: &String) -> usize { s.len() } // borrow, not move
   let s1 = String::from("hello");
   let n = calculate_length(&s1);
   println!("{s1} {n}"); // s1 still valid
   ```

2. **Restructure ownership** so the function takes ownership only when it really keeps the value. Return the value back if the caller needs it after.
   ```rust
   fn process(s: String) -> String { /* mutate */ s }
   let mut s = String::from("hello");
   s = process(s); // ownership returned
   ```

3. **`std::mem::take(&mut x)`** for types implementing `Default`. Leaves `x = T::default()` so `x` is still initialised.
   ```rust
   use std::mem;
   let mut v = vec![1, 2, 3];
   let taken = mem::take(&mut v); // v is now empty Vec, taken owns [1,2,3]
   ```

4. **`std::mem::replace(&mut x, new)`** swaps out the value, gives you the old one. Use when you must put a specific replacement, not `Default`.
   ```rust
   use std::mem;
   let mut s = String::from("old");
   let old = mem::replace(&mut s, String::from("new")); // s = "new", old = "old"
   ```

5. **`.clone()`** ONLY when (a) the data is small / `Copy`-ish, (b) you genuinely need two independent owners, and (c) you have already considered patterns 1 to 4. Document why a clone is correct, not a workaround.

NEVER add `.clone()` without first asking "can I borrow instead". `.clone()` on a `String` or `Vec` copies the heap allocation; `clippy::redundant_clone` will flag obvious abuses.

### After-NLL improvement

The error is reported at the **use** site, after NLL (Rust 2018+). The compiler often suggests the exact line to clone or borrow. ALWAYS read the suggestion before guessing.

---

## E0502: cannot borrow as mutable because also borrowed as immutable

**Official summary (verbatim):** *"A variable already borrowed with a certain mutability (either mutable or immutable) was borrowed again with a different mutability."* Source: `rustc_error_codes/E0502.md`.

### Root cause

Rust's rule: at any point, **either one `&mut` xor any number of `&`** to the same value. Two overlapping borrows of different mutability violate this.

### Erroneous example

```rust
fn bar(x: &mut i32) {}
fn foo(a: &mut i32) {
    let y = &a;     // immutable borrow of a
    bar(a);         // E0502: cannot borrow `*a` as mutable because also borrowed as immutable
    println!("{y}");
}
```

### Fix patterns

1. **Tighten the shared borrow scope (NLL).** Use the shared borrow, finish using it, *then* reborrow as `&mut`. NLL ends a borrow at its last use, not at the closing brace.
   ```rust
   fn foo(a: &mut i32) {
       let y = &a;
       println!("{y}");  // last use of y, borrow ends here
       bar(a);           // OK, now mutable borrow is fresh
   }
   ```

2. **Extract the read into a local** so the `&` borrow ends before the `&mut` begins.
   ```rust
   let len = vec.len();          // borrow ends after `len` is bound (Copy)
   vec.push(len);                // mutable borrow OK
   ```

3. **Restructure to avoid simultaneous borrow.** Read first, write second; or write first, read second. Never both in flight.

4. **Two-phase borrows** already cover the common case `vec.push(vec.len())` since Rust 1.18. If your code looks like this and still fails, the issue is more subtle (often E0499); see that section.

NEVER reach for `Arc` / `RefCell` to evade E0502. Those tools exist for genuine shared-ownership and interior-mutability problems, not for scope errors.

---

## E0596: cannot borrow as mutable, as it is not declared as mutable

**Official summary (verbatim):** *"This error occurs because you tried to mutably borrow a non-mutable variable."* Source: `rustc_error_codes/E0596.md`.

### Root cause

A binding declared without `mut` cannot be borrowed `&mut`. A method declared `&self` cannot mutate the receiver.

### Erroneous examples

```rust
let x = 1;
let y = &mut x; // E0596: cannot borrow `x` as mutable
```

```rust
struct Counter { n: u32 }
impl Counter {
    fn bump(&self) { self.n += 1; } // E0596: `self` is &, not &mut
}
```

### Fix patterns

1. **Add `mut` to the binding.**
   ```rust
   let mut x = 1;
   let y = &mut x; // OK
   ```

2. **Change `&self` to `&mut self`** on the method that mutates.
   ```rust
   impl Counter {
       fn bump(&mut self) { self.n += 1; }
   }
   ```

3. **If the receiver must stay `&self`** (e.g. trait method signature), use **interior mutability**: `Cell<T>` (for `Copy`), `RefCell<T>` (single-threaded, runtime-checked), `Mutex<T>` / `RwLock<T>` (thread-safe). See [[rust-syntax-borrowing]].
   ```rust
   use std::cell::Cell;
   struct Counter { n: Cell<u32> }
   impl Counter {
       fn bump(&self) { self.n.set(self.n.get() + 1); }
   }
   ```

NEVER promote a binding to `Arc<Mutex<T>>` just to fix E0596 in a single-threaded path. Use `Cell` or `RefCell`, or add `mut`. Over-engineering hides intent.

---

## E0499: cannot borrow as mutable more than once at a time

**Official summary (verbatim):** *"A variable was borrowed as mutable more than once."* Source: `rustc_error_codes/E0499.md`.

### Root cause

Two `&mut` references to the same value overlap in time. Rust forbids this because mutable aliasing is unsound.

### Erroneous example

```rust
let mut i = 0;
let x = &mut i;
let a = &mut i;  // E0499: cannot borrow `i` as mutable more than once
x;
```

### Fix patterns

1. **Scope-tighten via NLL.** Finish using the first `&mut` before creating the second. Many naive cases vanish.
   ```rust
   let mut i = 0;
   { let x = &mut i; *x += 1; }  // first borrow ends here
   let a = &mut i;               // OK
   ```

2. **Split borrow across disjoint struct fields.** The compiler tracks paths; `&mut s.a` and `&mut s.b` are allowed when `a` and `b` are distinct fields.
   ```rust
   struct S { a: u32, b: u32 }
   let mut s = S { a: 0, b: 0 };
   let pa = &mut s.a;
   let pb = &mut s.b; // OK, disjoint paths
   *pa = 1; *pb = 2;
   ```

3. **Split borrow on a slice via `split_at_mut`** for two disjoint halves.
   ```rust
   let mut v = vec![1, 2, 3, 4];
   let (left, right) = v.split_at_mut(2);
   left[0] = 10; right[0] = 30;
   ```

4. **Restructure ownership tree.** If the same `&mut` needs to flow into two consumers, hand it to one, finish, hand to the next. Sequence the work.

NEVER hold the first `&mut` longer than necessary to extend its lifetime past the conflict; that is the opposite direction. The fix is *shorter* borrows, not longer.

---

## E0500: closure requires unique access but it is already borrowed

**Official summary (verbatim):** *"A borrowed variable was used by a closure."* Source: `rustc_error_codes/E0500.md`.

### Root cause

A closure captures a value by unique (`&mut`) reference, but the same value is already borrowed (shared or mutable) elsewhere in scope.

### Erroneous example

```rust
fn you_know_nothing(jon_snow: &mut i32) {
    let nights_watch = &jon_snow;     // shared borrow
    let starks = || { *jon_snow = 3; }; // E0500: closure wants unique access
    println!("{nights_watch}");
}
```

### Fix patterns

1. **Drop the conflicting borrow before the closure is created (or first called).**
   ```rust
   fn you_know_nothing(jon_snow: &mut i32) {
       let nights_watch = &jon_snow;
       println!("{nights_watch}");   // borrow ends
       let starks = || { *jon_snow = 3; };
       starks();
   }
   ```

2. **`move` keyword to force capture by value.** Use when the closure should own the captured data and no outer borrow is needed.
   ```rust
   let v = vec![1, 2, 3];
   let print_v = move || println!("{v:?}");
   ```
   `move` does not fix overlapping borrow of the outer variable; it sidesteps the problem by transferring ownership.

3. **Restructure capture set.** Capture only the field you need, not the whole struct. Closures see the path you actually use.
   ```rust
   struct S { a: i32, b: i32 }
   let mut s = S { a: 0, b: 0 };
   let r_b = &s.b;             // disjoint capture
   let mut f = || s.a += 1;    // OK, captures only s.a
   f();
   println!("{r_b}");
   ```

4. **Clone for genuinely independent users.** If a closure must run later with a snapshot of the value, clone before capturing.

NEVER pass `&mut` to a closure and then read the outer variable while the closure is live. The closure holds the unique reference until dropped.

---

## E0716: temporary value dropped while borrowed

**Official summary (verbatim):** *"A temporary value is being dropped while a borrow is still in active use."* Source: `rustc_error_codes/E0716.md`.

### Root cause

`&expr` where `expr` is a function call, arithmetic result, or other rvalue creates an **anonymous temporary**. The temporary lives only until the end of the enclosing statement (the rules are in the Reference under "Temporary lifetimes"). If a borrow of that temporary tries to outlive the statement, E0716 fires.

### Erroneous example

```rust
fn foo() -> i32 { 22 }
fn bar(x: &i32) -> &i32 { x }
let p = bar(&foo());  // &foo() creates a temporary that dies after this statement
let q = *p;           // E0716: temporary dropped while borrowed
```

### Fix patterns

1. **`let`-bind the temporary** to extend its lifetime to the end of the enclosing block.
   ```rust
   let value = foo();      // owns the i32 for the whole block
   let p = bar(&value);
   let q = *p;             // OK
   ```

2. **`let tmp = &expr;`** also extends the temporary's lifetime to the enclosing block via temporary-lifetime-extension rules.
   ```rust
   let p = &foo();  // temporary extended to block scope
   let q = *p;
   ```

3. **Restructure so the call result is consumed within the same statement** if you do not need it later.
   ```rust
   println!("{}", bar(&foo()));  // single statement, no later use of p
   ```

NEVER `Box::leak(Box::new(expr))` to silence E0716. That leaks memory permanently. Use `let`-binding.

---

## Cross-cutting fix patterns

These apply to multiple error codes. Try in this order when the specific section above does not match:

| Pattern | Use when |
|---|---|
| Borrow instead of move | E0382 root pattern. Function reads or writes but does not consume. |
| Introduce intermediate binding | E0502 + E0716. Extracts a value out of a borrow so the borrow can end. |
| Scope-tighten via NLL | E0502 + E0499 + E0500. The borrow ends at last use; restructure so the second access is past that point. |
| Split borrow (struct fields or `split_at_mut`) | E0499 across disjoint paths. |
| `mem::take` / `mem::replace` | E0382 when you need ownership but cannot move. |
| Interior mutability (`Cell` / `RefCell`) | E0596 when receiver must stay `&self`. |
| `move` closure | E0500 when closure should own the data. |
| `let`-bind to extend temporary | E0716 verbatim fix. |
| `.clone()` | Last resort. ONLY when the alternatives fail or the value is small / cheap. |

---

## Anti-patterns (never do this)

See `references/anti-patterns.md` for full WHY explanations. Headlines:

1. **Cloning everything to silence E0382.** Doubles heap cost. Hides the real ownership question.
2. **`RefCell` to bypass E0502 / E0499.** Pushes a compile-time error into a runtime panic.
3. **`Arc<Mutex<T>>` to fix E0596 in single-threaded code.** Adds atomic cost for zero benefit.
4. **`Box::leak` to extend a temporary in E0716.** Permanent memory leak.
5. **`unsafe` + raw pointer to dodge the borrow checker.** Undefined behavior risk; soundness now on you.
6. **Holding a `MutexGuard` past its needed scope** to silence E0499. Extends critical section, hurts contention.

---

## Reference files

- `references/methods.md`: complete fix-recipe catalog with API signatures (`mem::take`, `mem::replace`, `split_at_mut`, `Cell`, `RefCell`, two-phase borrow rules, NLL spec).
- `references/examples.md`: side-by-side broken-vs-fixed for every error code, plus realistic Vec / HashMap / struct patterns.
- `references/anti-patterns.md`: each anti-pattern enumerated with broken code, why it compiles, why it harms, and the correct alternative.

---

## Sources

- Rust Compiler Error Index: https://doc.rust-lang.org/error_codes/error-index.html
- E0382: https://doc.rust-lang.org/error_codes/E0382.html
- E0502: https://doc.rust-lang.org/error_codes/E0502.html
- E0596: https://doc.rust-lang.org/error_codes/E0596.html
- E0499: https://doc.rust-lang.org/error_codes/E0499.html
- E0500: https://doc.rust-lang.org/error_codes/E0500.html
- E0716: https://doc.rust-lang.org/error_codes/E0716.html
- The Rust Book, Ch. 4 References and Borrowing: https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html
- `std::mem`: https://doc.rust-lang.org/std/mem/index.html
- `std::cell`: https://doc.rust-lang.org/std/cell/index.html
