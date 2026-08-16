---
name: rust-core-type-system
description: >
  Use when the user needs to understand Rust's type system fundamentals: nominal typing, zero-sized types, repr attributes, niche optimization, primitive types, the never type, type-state pattern, or const generics.
  Prevents misusing repr(C) when default repr suffices, packing types unsafely, assuming structural typing, or surprises with edition-2024 never-type fallback.
  Covers: nominal vs structural, ZST + PhantomData, repr (Rust/C/transparent/packed/align), niche optimization (NonZero, Option<NonNull>), primitives (integers/floats/bool/char/unit), never type `!` and edition-2024 fallback change, type-state pattern, const generics overview.
  Keywords: type system, nominal typing, zero-sized type, ZST, PhantomData, repr C, repr transparent, repr packed, niche optimization, NonZero, never type, "!", bottom type, divergence, type state, const generics, primitive types, "what is unit", "how big is", layout, alignment, "Option<NonZeroU32> size", "what is a tag", edition 2024 fallback.
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# Rust Core: Type System

Conceptual map of how Rust types are identified, laid out in memory, optimized, and composed. Read this before reaching for `#[repr(C)]`, `PhantomData`, `NonZero*`, or the never type `!`.

For const-generic mechanics see [[rust-syntax-generics]]. For the edition-2024 never-type fallback semantics see [[rust-syntax-edition-2024]]. For repr(C) in FFI bindings see [[rust-impl-ffi-bindgen]].

## Quick reference

| Concept | One-liner | Use when |
|---------|-----------|----------|
| Nominal typing | Identity is by name, not shape | ALWAYS expect newtype `struct Meters(f64)` to be distinct from `struct Seconds(f64)` |
| ZST | `()`, `PhantomData<T>`, `struct Unit;` occupy 0 bytes | Marker types, type-state, encoding info at zero runtime cost |
| `#[repr(Rust)]` (default) | Compiler chooses layout, may reorder fields | ALL pure-Rust code (best memory packing, no FFI) |
| `#[repr(C)]` | C layout, declaration order, predictable padding | FFI boundary, struct shared with C/C++ |
| `#[repr(transparent)]` | Single non-ZST field, same ABI as that field | Newtype wrappers that must be ABI-identical to inner |
| `#[repr(packed(N))]` | Disable padding (DANGEROUS) | Network/binary protocols with explicit alignment rules; never take `&field` |
| `#[repr(align(N))]` | Force higher alignment | Cache-line padding, hardware alignment requirements |
| `#[repr(u8)]` on enum | Stable discriminant size | FFI enums, on-wire encoding |
| Niche optimization | Invalid bit patterns encode `None` | `Option<NonZeroU32>` = 4 bytes (not 8), `Option<&T>` = pointer size |
| Never type `!` | Empty type, coerces to any other type | Return type of `panic!`, `loop {}`, `return`, `continue` |
| Type-state pattern | `PhantomData<State>` encodes state at compile time | Builders requiring ordered configuration; resources with lifecycle |
| Const generics | Type parameters that are values, e.g. `<const N: usize>` | Fixed-size arrays, dimension-tagged matrices |

## Decision trees

### Which `repr` do I need?

```
Is this struct/enum crossing an FFI boundary?
├── No  : Use default repr (no annotation). Compiler optimizes layout.
└── Yes
    ├── Is it a newtype wrapping ONE non-ZST field with same ABI?
    │   └── Use #[repr(transparent)]
    ├── Is it a multi-field struct shared with C?
    │   └── Use #[repr(C)]
    ├── Is it a field-less enum with stable discriminant?
    │   └── Use #[repr(u8)] / #[repr(i32)] / etc.
    └── Are you implementing a wire/binary protocol with explicit byte layout?
        └── Use #[repr(C, packed(N))]. NEVER take references to fields.
```

### When do I reach for `PhantomData<T>`?

```
Does the struct's type signature have a generic parameter T it does NOT store?
├── No  : Don't add PhantomData.
└── Yes : You need to express the relationship to T anyway. Use PhantomData<T>.
    ├── Want covariance + drop check? -> PhantomData<T>
    ├── Want invariance? -> PhantomData<fn(T) -> T>
    ├── Want contravariance? -> PhantomData<fn(T) -> ()>
    └── Want !Send / !Sync opt-out? -> PhantomData<*mut ()>
```

### When does niche optimization apply?

```
Is the inner type guaranteed to have an invalid bit pattern?
├── NonZero* (NonZeroU8..NonZeroU128, NonZeroUsize, NonZeroI*) : YES, zero is the niche
├── &T, &mut T, Box<T>, fn pointers                            : YES, null is the niche
├── NonNull<T>                                                  : YES, null is the niche
├── char                                                        : YES, surrogate range + values >0x10FFFF are niches
├── bool                                                        : YES, only 0 and 1 are valid
└── Plain u32, i32, f32                                         : NO, every bit pattern is valid
```

If yes : `Option<T>` and `Result<T, ZST>` have the same size as `T`.
If no  : `Option<T>` is at least `T` + 1 byte (likely more due to alignment padding).

## Patterns

### Pattern : Nominal typing for unit safety

```rust
// Two structs, identical shape, DIFFERENT TYPES. Compiler enforces conversion.
struct Meters(f64);
struct Feet(f64);

fn distance_in_meters(d: Meters) -> Meters { d }
// distance_in_meters(Feet(10.0));  // ERROR : expected Meters, found Feet
```

ALWAYS prefer newtype wrappers over raw primitives at API boundaries. NEVER pass `f64` between modules when the value has units, IDs, or domain constraints.

### Pattern : ZST as marker

```rust
struct LoggingEnabled;
struct LoggingDisabled;

struct Logger<S> {
    target: String,
    _state: std::marker::PhantomData<S>,
}

impl Logger<LoggingDisabled> {
    fn enable(self) -> Logger<LoggingEnabled> {
        Logger { target: self.target, _state: std::marker::PhantomData }
    }
}

impl Logger<LoggingEnabled> {
    fn log(&self, msg: &str) { println!("[{}] {}", self.target, msg); }
}
```

`size_of::<PhantomData<T>>() == 0`. The state is enforced entirely at compile time, with zero runtime cost. Per [std::marker::PhantomData][phantom-doc] : `align_of::<PhantomData<T>>() == 1`.

### Pattern : `repr(transparent)` newtype for FFI

```rust
#[repr(transparent)]
pub struct FileDescriptor(i32);

// Same ABI as i32. Safe to pass to C functions expecting int.
// Same niche behaviour as i32 (none, so Option<FileDescriptor> is larger).
```

ALWAYS use `#[repr(transparent)]` for FFI newtypes that wrap a single primitive. NEVER use `#[repr(C)]` for single-field newtypes when you want to preserve the inner type's ABI (repr(C) gives the struct C-struct ABI, which differs from primitive ABI on some platforms).

### Pattern : Niche-optimized handles

```rust
use std::num::NonZeroU32;

pub struct EntityId(NonZeroU32);
// Option<EntityId> is 4 bytes, NOT 8 bytes.
// `None` is encoded as the bit pattern 0, which is invalid for NonZeroU32.

impl EntityId {
    pub fn new(raw: u32) -> Option<Self> {
        NonZeroU32::new(raw).map(EntityId)
    }
}
```

Per [std::num::NonZeroU32][nonzero-doc] : `size_of::<Option<NonZeroU32>>() == size_of::<u32>()`. ALWAYS prefer `NonZeroU32` (or `NonZeroUsize`) for IDs and handles where 0 is invalid. The compiler reclaims the zero bit pattern for the `None` discriminant.

### Pattern : Never type for diverging functions

```rust
fn fatal(msg: &str) -> ! {
    eprintln!("FATAL: {msg}");
    std::process::exit(1);
}

let x: i32 = match parse_input() {
    Ok(n) => n,
    Err(_) => fatal("bad input"),  // ! coerces to i32 (or anything)
};
```

The never type `!` is the type of expressions that never produce a value. It coerces to any other type, which is what makes `panic!`, `loop {}`, `return`, and `continue` usable in arbitrary positions.

### Pattern : Type-state builder

```rust
use std::marker::PhantomData;

pub struct NeedsName;
pub struct NeedsAge;
pub struct Ready;

pub struct UserBuilder<State> {
    name: Option<String>,
    age: Option<u32>,
    _state: PhantomData<State>,
}

impl UserBuilder<NeedsName> {
    pub fn new() -> Self {
        Self { name: None, age: None, _state: PhantomData }
    }
    pub fn name(self, name: String) -> UserBuilder<NeedsAge> {
        UserBuilder { name: Some(name), age: self.age, _state: PhantomData }
    }
}

impl UserBuilder<NeedsAge> {
    pub fn age(self, age: u32) -> UserBuilder<Ready> {
        UserBuilder { name: self.name, age: Some(age), _state: PhantomData }
    }
}

impl UserBuilder<Ready> {
    pub fn build(self) -> User { User { name: self.name.unwrap(), age: self.age.unwrap() } }
}

pub struct User { name: String, age: u32 }

// Compile-time enforcement : .build() unreachable until .name() and .age() called in order.
```

## Primitive types reference

| Type | Size | Niche | Notes |
|------|------|-------|-------|
| `i8`..`i128` | 1..16 bytes | No | Two's complement |
| `isize` | pointer-sized | No | Platform dependent |
| `u8`..`u128` | 1..16 bytes | No | Unsigned |
| `usize` | pointer-sized | No | Index type for slices/arrays |
| `f32`, `f64` | 4, 8 bytes | No | IEEE 754 |
| `bool` | 1 byte | Yes (only 0/1 valid) | `Option<bool>` = 1 byte |
| `char` | 4 bytes | Yes | Unicode scalar value, not UTF-8 code unit |
| `()` (unit) | 0 bytes | n/a | ZST, sole inhabitant `()` |
| `!` (never) | 0 bytes | n/a | No inhabitants, coerces to any type |

`char` is ALWAYS 4 bytes. NEVER conflate `char` with a UTF-8 byte (use `u8` for raw bytes or iterate `str::chars()` for scalars).

## Edition 2024 : never-type fallback change

CRITICAL behavioural change in edition 2024 (Rust 1.85+). Before edition 2024, type inference allowed `!` to fall back to `()`. In edition 2024, it falls back to `!` itself.

```rust
// Edition 2021 behaviour : T inferred as ()
// Edition 2024 behaviour : T inferred as ! -> compile error if ! does not satisfy bounds
fn outer<T>(x: T) -> Result<T, ()> {
    fn f<T: Default>() -> Result<T, ()> { Ok(T::default()) }
    f()?;       // Edition 2024 : T = !, but ! : Default not satisfied -> ERROR
    Ok(x)
}

// Fix : explicit turbofish
fn outer_fixed<T>(x: T) -> Result<T, ()> {
    fn f<T: Default>() -> Result<T, ()> { Ok(T::default()) }
    f::<()>()?; // explicit T = ()
    Ok(x)
}
```

ALWAYS migrate via explicit type annotations when upgrading to edition 2024. The `never_type_fallback_flowing_into_unsafe` lint is `deny` by default, catching the dangerous cases at compile time. See [[rust-syntax-edition-2024]] for the full migration checklist.

## Const generics overview

`<const N: usize>` parameterizes a type by a value, not another type. Stable for primitive integer types, `bool`, and `char`.

```rust
pub struct Matrix<const ROWS: usize, const COLS: usize> {
    data: [[f64; COLS]; ROWS],
}

impl<const R: usize, const C: usize> Matrix<R, C> {
    pub fn zero() -> Self { Self { data: [[0.0; C]; R] } }
}

let m: Matrix<3, 4> = Matrix::zero();
```

ALWAYS use const generics for fixed-size APIs (matrices, ring buffers, fixed-key crypto). NEVER attempt complex const expressions like `<const N: usize, const M: usize>` with `N + M` in bounds without the `generic_const_exprs` feature (still unstable). For const-generic depth see [[rust-syntax-generics]].

## Reference files

- [`references/methods.md`](references/methods.md) : concept surface (repr attributes table, primitive sizes, niche-eligible types, never-type rules)
- [`references/examples.md`](references/examples.md) : working code patterns (nominal typing, ZST markers, repr variants, niche optimization, type-state, const generics)
- [`references/anti-patterns.md`](references/anti-patterns.md) : 6 documented anti-patterns with root-cause analysis

## External sources

- [Rust Reference : types][ref-types]
- [Rust Reference : type-layout][ref-layout]
- [Rust Reference : special-types-and-traits][ref-special]
- [Rust Reference : never type][ref-never]
- [std::marker::PhantomData][phantom-doc]
- [std::num::NonZeroU32][nonzero-doc]
- [Edition 2024 : never-type-fallback][never-fallback]

[ref-types]: https://doc.rust-lang.org/reference/types.html
[ref-layout]: https://doc.rust-lang.org/reference/type-layout.html
[ref-special]: https://doc.rust-lang.org/reference/special-types-and-traits.html
[ref-never]: https://doc.rust-lang.org/reference/types/never.html
[phantom-doc]: https://doc.rust-lang.org/std/marker/struct.PhantomData.html
[nonzero-doc]: https://doc.rust-lang.org/std/num/type.NonZeroU32.html
[never-fallback]: https://doc.rust-lang.org/edition-guide/rust-2024/never-type-fallback.html
