# Type System : Working Examples

All examples compile against Rust 1.85+, edition 2024. Sources : Rust Reference (types, type-layout, never type), std docs (PhantomData, NonZeroU32, NonNull).

## 1. Nominal typing : two structs, same fields, different types

```rust
struct Meters(f64);
struct Feet(f64);

fn walk(distance: Meters) -> Meters {
    Meters(distance.0 + 1.0)
}

fn main() {
    let d = Meters(5.0);
    let _ = walk(d);

    // let bad = Feet(5.0);
    // walk(bad);
    // ERROR : expected `Meters`, found `Feet`
}
```

Rust will NEVER auto-convert `Feet` to `Meters` even though both wrap `f64`. The newtype pattern provides zero-cost type safety.

## 2. ZST : empty struct, unit, PhantomData

```rust
use std::marker::PhantomData;
use std::mem::{size_of, align_of};

struct Empty;
struct Marker<T> { _phantom: PhantomData<T> }

fn main() {
    assert_eq!(size_of::<()>(), 0);
    assert_eq!(size_of::<Empty>(), 0);
    assert_eq!(size_of::<PhantomData<u64>>(), 0);
    assert_eq!(align_of::<PhantomData<u64>>(), 1);

    // ZST inside Vec : capacity is virtual, no allocation occurs.
    let v: Vec<()> = vec![(); 1_000_000];
    assert_eq!(v.len(), 1_000_000);
    // size_of_val(&v[0]) == 0, no heap memory allocated for elements.
}
```

## 3. Default repr : compiler may reorder

```rust
struct Mixed {
    a: u8,    // 1 byte
    b: u64,   // 8 bytes
    c: u8,    // 1 byte
}
// Default repr : compiler reorders to {b, a, c} with packing.
// Total size : likely 16 bytes (8 for b, 2 for a + c with padding).

// With repr(C) : declaration order preserved.
#[repr(C)]
struct MixedC {
    a: u8,    // offset 0, 1 byte
    // 7 bytes padding
    b: u64,   // offset 8, 8 bytes
    c: u8,    // offset 16, 1 byte
    // 7 bytes padding
}
// Total size : 24 bytes.
```

ALWAYS use default repr for pure-Rust code. The compiler packs better. Only switch to `#[repr(C)]` when external code reads the bytes.

## 4. `#[repr(transparent)]` : ABI-identical newtype

```rust
#[repr(transparent)]
pub struct Pid(i32);

impl Pid {
    pub fn new(raw: i32) -> Self { Pid(raw) }
    pub fn as_raw(&self) -> i32 { self.0 }
}

// Safe to pass to C function expecting `int`.
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

// Equivalent ABI : caller can pass Pid where i32 is expected (with appropriate transmute or `as_raw`).
```

`#[repr(transparent)]` is the right choice for FFI newtype wrappers. `#[repr(C)]` would give the struct a C-struct ABI which differs from primitive ABI on some platforms.

## 5. `#[repr(C)]` : multi-field FFI struct

```rust
#[repr(C)]
pub struct timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

extern "C" {
    fn clock_gettime(clk_id: i32, tp: *mut timespec) -> i32;
}

fn now() -> timespec {
    let mut ts = timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { clock_gettime(0, &mut ts as *mut _); }
    ts
}
```

The struct's bytes match the C definition exactly. ALWAYS use `#[repr(C)]` when sharing struct layout with C.

## 6. `#[repr(packed)]` : binary protocol, safely

```rust
#[repr(C, packed)]
struct EthernetHeader {
    dst_mac: [u8; 6],
    src_mac: [u8; 6],
    ethertype: u16,  // misaligned : offset 12, but alignment of u16 is 2 (ok here actually)
}

fn parse(bytes: &[u8; 14]) -> u16 {
    let header = unsafe { &*(bytes.as_ptr() as *const EthernetHeader) };

    // SAFE : copy out the field to a local, then read.
    let et = header.ethertype;
    et.to_be()

    // UNSAFE / UB : let r = &header.ethertype; // alignment violation
    //
    // CORRECT alternative using raw pointer methods (Rust 1.82+) :
    // let ptr: *const u16 = &raw const header.ethertype;
    // let et = unsafe { ptr.read_unaligned() };
}
```

NEVER take `&header.field` on a packed struct. Copy the value out, or use `&raw const` / `&raw mut` with `read_unaligned` / `write_unaligned`.

## 7. Niche optimization : `Option<NonZeroU32>` is 4 bytes

```rust
use std::num::NonZeroU32;
use std::mem::size_of;

fn main() {
    assert_eq!(size_of::<u32>(), 4);
    assert_eq!(size_of::<NonZeroU32>(), 4);
    assert_eq!(size_of::<Option<NonZeroU32>>(), 4);
    // None is encoded as the bit pattern 0, which is invalid for NonZeroU32.

    // Without niche optimization :
    assert_eq!(size_of::<Option<u32>>(), 8);
    // 4 bytes for u32 + 4 bytes for discriminant (with alignment padding).
}
```

## 8. Niche optimization : `Option<Box<T>>` is pointer-sized

```rust
use std::mem::size_of;

fn main() {
    assert_eq!(size_of::<Box<i64>>(), size_of::<*const i64>());
    assert_eq!(size_of::<Option<Box<i64>>>(), size_of::<*const i64>());
    // None = null pointer. Box is guaranteed non-null, so null is the niche.
}
```

## 9. Niche optimization : `Option<&T>` is pointer-sized

```rust
fn first<'a>(slice: &'a [i32]) -> Option<&'a i32> {
    slice.first()
}
// size_of returned value : one pointer (8 bytes on 64-bit). Not 16.
```

## 10. Never type `!` in match arms

```rust
fn fatal(msg: &str) -> ! {
    eprintln!("FATAL: {msg}");
    std::process::exit(1);
}

fn parse_or_die(s: &str) -> i32 {
    match s.parse::<i32>() {
        Ok(n) => n,
        Err(_) => fatal("not a number"),  // ! coerces to i32
    }
}
```

`fatal` returns `!`. The match arms must have the same type; `!` coerces to whatever the other arm produces.

## 11. Never type `!` in loops

```rust
fn run_forever() -> ! {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

// The function is declared to return !, and the loop never breaks, so it satisfies the contract.
```

## 12. Edition 2024 : never-type fallback breaks code

```rust
// THIS COMPILES IN 2021, FAILS IN 2024 :
//
// fn outer<T>(x: T) -> Result<T, ()> {
//     fn f<T: Default>() -> Result<T, ()> { Ok(T::default()) }
//     f()?;
//     Ok(x)
// }
//
// 2024 ERROR : the trait `Default` is not implemented for `!`

// FIX : explicit turbofish.
fn outer<T>(x: T) -> Result<T, ()> {
    fn f<T: Default>() -> Result<T, ()> { Ok(T::default()) }
    f::<()>()?;
    Ok(x)
}
```

ALWAYS annotate inference points that previously relied on the `! -> ()` fallback when upgrading to edition 2024.

## 13. Type-state pattern : compile-time state machine

```rust
use std::marker::PhantomData;

pub struct Open;
pub struct Closed;

pub struct Door<S> {
    label: String,
    _state: PhantomData<S>,
}

impl Door<Closed> {
    pub fn new(label: String) -> Self {
        Door { label, _state: PhantomData }
    }
    pub fn open(self) -> Door<Open> {
        Door { label: self.label, _state: PhantomData }
    }
}

impl Door<Open> {
    pub fn close(self) -> Door<Closed> {
        Door { label: self.label, _state: PhantomData }
    }
    pub fn walk_through(&self) { println!("walking through {}", self.label); }
}

fn main() {
    let d = Door::<Closed>::new("front".to_string());
    // d.walk_through();  // ERROR : method only on Door<Open>
    let d = d.open();
    d.walk_through();
    let _d = d.close();
}
```

`walk_through` is only callable on `Door<Open>`. The compiler enforces lifecycle correctness, with zero runtime cost (`Open` and `Closed` are ZSTs).

## 14. Const generics : matrix with compile-time dimensions

```rust
pub struct Matrix<const R: usize, const C: usize> {
    data: [[f64; C]; R],
}

impl<const R: usize, const C: usize> Matrix<R, C> {
    pub fn zero() -> Self { Matrix { data: [[0.0; C]; R] } }
    pub fn identity() -> Self where /* R == C ideally, but const eq is unstable */ Self: Sized {
        let mut m = Self::zero();
        let mut i = 0;
        while i < R && i < C {
            m.data[i][i] = 1.0;
            i += 1;
        }
        m
    }
}

fn main() {
    let _m3x4: Matrix<3, 4> = Matrix::zero();
    let _id3: Matrix<3, 3> = Matrix::identity();
}
```

`R` and `C` are values, not types. The compiler monomorphizes a separate `Matrix<3, 4>` and `Matrix<3, 3>` from the same generic definition.

## 15. Variance via PhantomData

```rust
use std::marker::PhantomData;

// Covariant in T : Container<&'static str> is a subtype of Container<&'short str>.
struct Covariant<T>(PhantomData<T>);

// Invariant in T : neither subtype relationship holds.
struct Invariant<T>(PhantomData<fn(T) -> T>);

// Contravariant in T : Container<&'short str> is a subtype of Container<&'static str>.
struct Contravariant<T>(PhantomData<fn(T) -> ()>);

// !Send + !Sync via raw pointer phantom.
struct NotThreadSafe(PhantomData<*mut ()>);

fn assert_send<T: Send>() {}
// assert_send::<NotThreadSafe>();  // ERROR : NotThreadSafe is !Send
```

Use `PhantomData<*mut ()>` to explicitly opt out of `Send` and `Sync` when your struct's safety contract is single-threaded.
