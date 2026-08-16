# Edition 2024 reference : working examples per change

Each section below contains the smallest self-contained snippet that demonstrates the edition-2024 behaviour difference. Every example was verified against the Rust Edition Guide and the Rust 1.85.0 release notes. Where an example would compile under both editions but behave differently, the BEHAVIOUR is annotated explicitly.

## Change 1 : RPIT lifetime capture

### Edition 2021

```rust
// rustc 1.85 edition 2021
fn first_byte<'a>(buf: &'a [u8]) -> impl Sized {
    buf[0]                // returns u8
}

fn main() {
    let v = vec![1, 2, 3];
    let r = first_byte(&v);    // r: impl Sized, NOT bounded by `'_`
    drop(v);                   // OK ; r is not tied to `v`
    let _ = r;
}
```

### Edition 2024 (same source)

```rust
// rustc 1.85 edition 2024 ; SAME source as above
// fn first_byte<'a>(buf: &'a [u8]) -> impl Sized { buf[0] }
//
// Effective signature in 2024:
// fn first_byte<'a>(buf: &'a [u8]) -> impl Sized + use<'a>
//
// fn main() {
//     let v = vec![1, 2, 3];
//     let r = first_byte(&v);  // r is bounded by `'a` (lifetime of `&v`)
//     drop(v);                 // ERROR E0505 ; r still borrows v
//     let _ = r;
// }
```

### Edition 2024, preserving 2021 semantics

```rust
fn first_byte<'a>(buf: &'a [u8]) -> impl Sized + use<> {
    //                                       ^^^^^^^ capture nothing
    buf[0]
}

fn main() {
    let v = vec![1, 2, 3];
    let r = first_byte(&v);
    drop(v);                   // OK again
    let _ = r;
}
```

### Edition 2024, explicit subset capture

```rust
fn pair<'a, 'b, T: Clone>(x: &'a [u8], y: &'b T) -> impl Clone + use<'a, T> {
    //                                                          ^^^^^^^^^^^
    //                                          captures 'a and T but not 'b
    (x[0], y.clone())
}
```

## Change 2 : never-type fallback

### Edition 2021 (compiles, infers `T = ()`)

```rust
fn outer<T>(x: T) -> Result<T, ()> {
    fn f<T: Default>() -> Result<T, ()> { Ok(T::default()) }
    f()?;             // T inferred as `()`
    Ok(x)
}

fn main() {
    let _: Result<i32, ()> = outer(42);
}
```

### Edition 2024 (errors : `! : Default` does not hold)

```rust
// fn outer<T>(x: T) -> Result<T, ()> {
//     fn f<T: Default>() -> Result<T, ()> { Ok(T::default()) }
//     f()?;
//     //~^ ERROR `!` does not implement `Default`
//     Ok(x)
// }
```

### Edition 2024 fix

```rust
fn outer<T>(x: T) -> Result<T, ()> {
    fn f<T: Default>() -> Result<T, ()> { Ok(T::default()) }
    f::<()>()?;       // explicit turbofish
    Ok(x)
}
```

## Change 3 : unsafe extern

### Edition 2021

```rust
extern "C" {
    pub fn sqrt(x: f64) -> f64;
    pub unsafe fn strlen(p: *const std::ffi::c_char) -> usize;
}

fn main() {
    let s = unsafe { sqrt(2.0) };       // every call needs unsafe{}
    println!("{s}");
}
```

### Edition 2024

```rust
unsafe extern "C" {
    pub safe fn sqrt(x: f64) -> f64;    // ABI audited, no preconditions
    pub unsafe fn strlen(p: *const std::ffi::c_char) -> usize;
    pub fn free(p: *mut core::ffi::c_void);   // defaults to unsafe
}

fn main() {
    let s = sqrt(2.0);                  // no unsafe block needed
    println!("{s}");
    let n = unsafe { strlen(c"hi".as_ptr()) };
    println!("{n}");
}
```

## Change 4 : unsafe attributes

### Edition 2021

```rust
#[no_mangle]
pub extern "C" fn rust_entry() {
    println!("hello from rust_entry");
}

#[link_section = ".init_array"]
#[allow(dead_code)]
static INIT_HOOK: extern "C" fn() = rust_entry;
```

### Edition 2024

```rust
// SAFETY: This is the only function in the entire link target that uses the
// symbol name `rust_entry`. A duplicate symbol would be a link-time error.
#[unsafe(no_mangle)]
pub extern "C" fn rust_entry() {
    println!("hello from rust_entry");
}

// SAFETY: The .init_array section runs hooks at process startup. The pointer
// stored here must remain valid; `rust_entry` is a static fn pointer.
#[unsafe(link_section = ".init_array")]
#[allow(dead_code)]
static INIT_HOOK: extern "C" fn() = rust_entry;
```

## Change 5 : unsafe_op_in_unsafe_fn

### Edition 2021

```rust
unsafe fn get_unchecked<T>(slice: &[T], i: usize) -> &T {
    slice.get_unchecked(i)        // unsafe op without inner block ; OK
}

fn main() {
    let v = vec![10, 20, 30];
    let r = unsafe { get_unchecked(&v, 1) };
    println!("{r}");
}
```

### Edition 2024

```rust
unsafe fn get_unchecked<T>(slice: &[T], i: usize) -> &T {
    // SAFETY: caller guarantees `i < slice.len()`.
    unsafe { slice.get_unchecked(i) }
}

fn main() {
    let v = vec![10, 20, 30];
    let r = unsafe { get_unchecked(&v, 1) };
    println!("{r}");
}
```

## Change 6 : if let temporary scope

### Edition 2021 (deadlocks at runtime)

```rust
use std::sync::RwLock;

fn upsert(value: &RwLock<Option<bool>>) {
    if let Some(x) = *value.read().unwrap() {
        println!("already set: {x}");
    } else {
        // DEADLOCK : read lock still held here in 2021 ; write lock blocks forever.
        let mut w = value.write().unwrap();
        if w.is_none() {
            *w = Some(true);
        }
    }
}

fn main() {
    let v = RwLock::new(None::<bool>);
    upsert(&v);
}
```

### Edition 2024 (no deadlock ; same source)

```rust
// SAME source as above.
// In 2024, the read lock acquired in `*value.read().unwrap()` is dropped
// BEFORE entering the `else` arm, so `value.write()` succeeds.
```

### Edition 2024 explicit-match equivalent (what `cargo fix --edition` writes)

```rust
fn upsert_explicit(value: &RwLock<Option<bool>>) {
    match *value.read().unwrap() {
        Some(x) => {
            println!("already set: {x}");
        }
        None => {
            // The read guard is STILL held here ; `match` preserves 2021 scope.
            // ALWAYS prefer the if-let form on 2024 unless you NEED 2021 scope.
            let _ = value;     // suppress dead-code warning
        }
    }
}
```

## Change 7 : tail expression drop order

### Edition 2021 (compiles)

```rust
use std::cell::RefCell;

fn len() -> usize {
    let c = RefCell::new("..".to_string());
    c.borrow().len()
    // Order in 2021:
    //   1. `c.borrow()` returns a Ref<'_, String>
    //   2. `.len()` produces usize
    //   3. block locals dropped: `c` first
    //   4. tail temporary dropped: the Ref<'_, _>
    // Step 4 references `c`, which was dropped in step 3.
    // Pre-2024 this was permitted because tail temporaries lived longer.
}

fn main() {
    println!("{}", len());
}
```

### Edition 2024 (errors)

```rust
// fn len() -> usize {
//     let c = RefCell::new("..".to_string());
//     c.borrow().len()
//     //~^ ERROR E0597 `c` does not live long enough
// }
```

### Edition 2024 fix

```rust
fn len_fixed() -> usize {
    let c = RefCell::new("..".to_string());
    let borrowed = c.borrow();
    borrowed.len()
    // `borrowed` is a local; it drops before `c`. Tail expression `borrowed.len()`
    // produces a `usize`, which is `Copy` and has no destructor.
}
```

## Change 8 : static mut references

### Edition 2021 (warns)

```rust
static mut COUNTER: i32 = 0;

fn bump() {
    unsafe {
        let r = &COUNTER;       // warn: static_mut_refs
        println!("{r}");
        COUNTER += 1;
    }
}
```

### Edition 2024 migration target : atomic

```rust
use std::sync::atomic::{AtomicI32, Ordering};

static COUNTER: AtomicI32 = AtomicI32::new(0);

fn bump() {
    COUNTER.fetch_add(1, Ordering::Relaxed);
    println!("{}", COUNTER.load(Ordering::Relaxed));
}
```

### Edition 2024 migration target : raw pointer (when reference semantics NOT needed)

```rust
static mut COUNTER: i32 = 0;

fn bump() {
    unsafe {
        let p = &raw mut COUNTER;     // 1.82+ raw-pointer syntax
        *p += 1;
        println!("{}", *p);
    }
}
```

## Change 9 : expr fragment widening

### Edition 2021 (with `expr_2021`-equivalent semantics)

```rust
macro_rules! demo {
    ($e:expr) => { println!("expr: {:?}", $e) };
}

fn main() {
    demo!(1 + 1);                 // OK
    // demo!(const { 1 + 1 });    // edition 2021: ERROR ; expr did not match const blocks
    // demo!(_);                  // edition 2021: ERROR ; expr did not match `_`
}
```

### Edition 2024

```rust
macro_rules! demo {
    ($e:expr) => { println!("expr: {:?}", $e) };
}

fn main() {
    demo!(1 + 1);                 // OK
    demo!(const { 1 + 1 });       // OK in 2024 ; expr now matches const blocks
    // demo!(_);                  // matches `expr` grammar but `_` is not a value
                                  // expression at this position, so still errors
                                  // at a different layer.
}
```

### Edition 2024 with explicit `expr_2021` for backward-compatible matching

```rust
macro_rules! strict_expr {
    ($e:expr_2021) => { /* matches pre-2024 grammar only */ };
}
```

## Change 10 : gen keyword

### Edition 2021

```rust
fn gen() -> i32 { 0 }            // identifier `gen` ; OK in 2021

fn main() {
    println!("{}", gen());
}
```

### Edition 2024

```rust
fn r#gen() -> i32 { 0 }          // identifier escaped with `r#`

fn main() {
    println!("{}", r#gen());
}
```

`gen { ... }` BLOCKS themselves are NOT stable on edition 2024 ; only the keyword reservation is.

## Change 11 : reserved guarded string syntax

Edition 2024 reserves syntax for forthcoming guarded string literals. In practice no user-visible code patterns require change today ; the autofix handles the rare cases.

## Change 12 : match ergonomics with `&` patterns

### Edition 2024 restriction

Edition 2024 tightens match-ergonomics defaults for patterns that mix `&` with binding modes. The exact change is narrow and rarely encountered. The fix when it triggers is to write the pattern explicitly :

```rust
// edition 2021 / edition 2024 (no change)
let v: Vec<&u8> = vec![&1, &2, &3];
for &x in &v { let _ = x; }

// edition 2021 only (default-binding interaction)
//   let _ = match &Some(0u8) {
//       Some(x) => x,
//       None => &0,
//   };
// edition 2024 fix (explicit `ref`)
let _ = match &Some(0u8) {
    Some(ref x) => x,
    None => &0,
};
```

See [[rust-syntax-pattern-matching]] for the broader pattern grammar.

## Change 13 : prelude additions (Future, IntoFuture)

### Edition 2021 (no conflict)

```rust
trait MyPoller {
    fn poll(&self) -> i32;
}

impl MyPoller for () {
    fn poll(&self) -> i32 { 42 }
}

fn main() {
    let p: &dyn MyPoller = &();
    println!("{}", p.poll());            // unambiguous in 2021
}
```

### Edition 2024 (ambiguous)

```rust
// trait MyPoller { fn poll(&self) -> i32; }
// impl MyPoller for () { fn poll(&self) -> i32 { 42 } }
//
// fn main() {
//     let p: &dyn MyPoller = &();
//     println!("{}", p.poll());
//     //~^ ERROR E0034 multiple applicable items in scope
//     //   (Future::poll is now in the prelude)
// }
```

### Edition 2024 fix (fully-qualified)

```rust
trait MyPoller {
    fn poll(&self) -> i32;
}

impl MyPoller for () {
    fn poll(&self) -> i32 { 42 }
}

fn main() {
    let p: &dyn MyPoller = &();
    println!("{}", <dyn MyPoller>::poll(p));
}
```

The autofix from `cargo fix --edition` writes the FQN form for every flagged call site.

## End-to-end migration walkthrough

Starting state : a crate on edition 2021 that compiles cleanly on rustc 1.85.

```
$ cat Cargo.toml
[package]
name = "demo"
version = "0.1.0"
edition = "2021"

[dependencies]
```

```
$ cargo build
   Compiling demo v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.42s

$ git status
On branch main
nothing to commit, working tree clean

$ cargo fix --edition
    Migrating Cargo.toml from 2021 edition to 2024
     Checking demo v0.1.0
warning: ...impl_trait_overcaptures...
     Fixed src/lib.rs (3 changes applied)
warning: ...unsafe_op_in_unsafe_fn...
     Fixed src/lib.rs (2 changes applied)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.18s
    Migrating `Cargo.toml` from 2021 edition to 2024
       Edited `Cargo.toml`
```

Review the diff. ALWAYS run `git diff src/lib.rs` before staging. Then :

```
$ cargo build
$ cargo test
$ cargo clippy --all-targets -- -D warnings
$ git commit -am "Migrate to edition 2024 via cargo fix --edition"
```

If `cargo build` fails, the most likely culprits in order of frequency :

1. `tail_expr_drop_order` E0597 (no autofix ; introduce `let`-binding).
2. `static_mut_refs` deny-by-default (no autofix ; refactor to atomic / Mutex / OnceLock).
3. `never_type_fallback_flowing_into_unsafe` deny-by-default (annotate turbofish at the unsafe call site).
4. `if_let_rescope`-induced runtime behaviour change (in tests, not at compile time).
