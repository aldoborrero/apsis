# Type System : Concept Surface

Reference tables for repr attributes, primitive types, niche-eligible types, and never-type rules. All claims verified against Rust Reference and `std` docs (2026-05-19).

## 1. `repr` attributes : complete table

| Attribute | Applies to | Layout guarantee | Field ordering | ABI | Typical use |
|-----------|------------|------------------|----------------|-----|-------------|
| `#[repr(Rust)]` (default, implicit) | struct, enum, union | None beyond alignment + non-overlap | Compiler may reorder | Rust ABI (unstable) | Pure-Rust code; allows compiler to pack optimally |
| `#[repr(C)]` | struct, enum, union | Declaration order, C-compatible padding | Declaration order | C ABI | FFI struct/enum shared with C/C++ |
| `#[repr(transparent)]` | struct, single-variant enum with one non-ZST field | Same layout/size/ABI as the one non-ZST field | Trivial | Same as inner | Newtype with identical ABI to wrapped type |
| `#[repr(packed)]` (= `packed(1)`) | struct, union | Removes ALL inter-field padding | No ordering guarantee alone | None | Wire/binary protocols with byte-exact layout |
| `#[repr(packed(N))]` where N ∈ {1,2,4,...,2²⁹} | struct, union | Aligns fields to `min(N, natural_align)` | No ordering guarantee alone | None | Mixed-alignment binary structs |
| `#[repr(align(N))]` where N ∈ {1,2,4,...,2²⁹} | struct, union, enum | Forces alignment ≥ N | No effect on ordering | Inherits | Cache-line padding (e.g. `align(64)`), hardware alignment |
| `#[repr(u8)]`, `#[repr(u16)]`, `#[repr(u32)]`, `#[repr(u64)]`, `#[repr(usize)]`, signed variants | enum | Discriminant has the specified integer type | n/a | Inherits | FFI enums, stable on-wire discriminants |

### Combination rules

- `#[repr(C, packed)]` : VALID. C ordering with no padding.
- `#[repr(C, align(N))]` : VALID. C ordering with extra alignment.
- `#[repr(packed, align(N))]` : INVALID. Cannot both pack and force higher alignment on the same type.
- `#[repr(transparent)]` : EXCLUSIVE. Cannot be combined with any other repr.

### `repr(transparent)` exact constraints

Applies only when the type has EXACTLY ONE non-zero-sized field. Any number of ZST fields with alignment 1 (e.g. `PhantomData<T>`) are permitted. Without any non-ZST field, the type acts as a unit (1-ZST). Source : Rust Reference, type-layout.

### `repr(packed)` safety rules

- Creating a reference (`&field` or `&mut field`) to a misaligned packed field is UNDEFINED BEHAVIOR.
- Use raw pointer methods : `&raw const field` then `ptr.read_unaligned()`, or `&raw mut field` then `ptr.write_unaligned(value)`.
- The `&raw const` / `&raw mut` syntax stabilized in Rust 1.82.
- Copy out via local variable : `let v = packed.field;` then operate on `v`.
- The compiler refuses to derive most traits (`Debug`, `Hash`, etc.) on `#[repr(packed)]` structs because the derive macro would generate field references.

## 2. Primitive types : size, alignment, niche

| Type | Size | Alignment | Niche-eligible | Notes |
|------|------|-----------|----------------|-------|
| `i8`, `u8` | 1 | 1 | No | Smallest integers |
| `i16`, `u16` | 2 | 2 | No | |
| `i32`, `u32` | 4 | 4 | No | Default integer when literal needs no context |
| `i64`, `u64` | 8 | 8 | No | |
| `i128`, `u128` | 16 | 16 (on most platforms) | No | Edition 2024 fixed ABI mismatch with C `_BitInt(128)` on x86-64 |
| `isize`, `usize` | pointer-sized | pointer-sized | No | Index type for `[T]`; 4 bytes on 32-bit, 8 on 64-bit |
| `f32` | 4 | 4 | No | IEEE 754 single precision |
| `f64` | 8 | 8 | No | IEEE 754 double precision |
| `bool` | 1 | 1 | Yes | Only 0 (false) and 1 (true) valid; other bit patterns UB |
| `char` | 4 | 4 | Yes | Unicode scalar value; surrogates D800..DFFF and values >0x10FFFF are niches |
| `()` (unit) | 0 | 1 | n/a | Sole inhabitant `()`; ZST |
| `!` (never) | 0 | 1 | n/a | No inhabitants; ZST; coerces to any type |
| `&T`, `&mut T` | pointer | pointer | Yes (null) | References are non-null by guarantee |
| `Box<T>` | pointer | pointer | Yes (null) | Owning heap pointer |
| `fn() -> T` | pointer | pointer | Yes (null) | Function pointer; non-null |

## 3. Niche-eligible types : the layout-guarantee promise

A type T has a "niche" when at least one bit pattern of `size_of::<T>()` bytes is INVALID as a T. The compiler reclaims that pattern to encode `None` or other discriminants.

### Guaranteed niche-bearing standard types

| Type | Niche value(s) | `Option<T>` size guarantee |
|------|----------------|----------------------------|
| `NonZeroU8`, `NonZeroI8` | 0 | Same as `T` (1 byte) |
| `NonZeroU16`, `NonZeroI16` | 0 | Same as `T` (2 bytes) |
| `NonZeroU32`, `NonZeroI32` | 0 | Same as `T` (4 bytes) |
| `NonZeroU64`, `NonZeroI64` | 0 | Same as `T` (8 bytes) |
| `NonZeroU128`, `NonZeroI128` | 0 | Same as `T` (16 bytes) |
| `NonZeroUsize`, `NonZeroIsize` | 0 | Same as `T` (pointer-sized) |
| `&T`, `&mut T` | null pointer | Same as `T` (pointer-sized) |
| `Box<T>` | null pointer | Same as `T` (pointer-sized) |
| `Rc<T>`, `Arc<T>` | null pointer (Rc/Arc are pointers) | Same as `T` (pointer-sized) |
| `NonNull<T>` | null pointer | Same as `T` (pointer-sized) |
| `fn() -> T`, `unsafe fn(...)` etc. | null | Same as `T` (pointer-sized) |
| `bool` | bit patterns other than 0 and 1 | Larger than `bool` only when `Option<bool>` happens to be 2 bytes due to padding, typically 1 byte |
| `char` | code points >0x10FFFF and D800..DFFF | Same as `char` (4 bytes) for `Option<char>` |

### Niche layering

`Option<Option<T>>` can stay the same size as `T` when T has multiple niches. For example, `Option<Option<NonZeroU8>>` is 1 byte : 0 = `None`, 1 = `Some(None)`, 2..255 = `Some(Some(NonZeroU8::new(n).unwrap()))`.

### Source guarantee

Per the std docs on `NonZeroU32` : "Thanks to the null pointer optimization, `NonZeroU32` and `Option<NonZeroU32>` are guaranteed to have the same size and alignment." Verified : `size_of::<Option<NonZeroU32>>() == size_of::<u32>()` and `size_of::<NonZeroU32>() == size_of::<Option<NonZeroU32>>()`.

## 4. Never type `!` : rules and edition-2024 fallback

### Core semantics

- `!` is the type with zero values (uninhabited / "bottom").
- Any expression of type `!` is *diverging* : it never produces a value.
- `!` coerces to any other type `T` (because the coercion is vacuous : there is no `!` value to convert).
- Returning `!` is how you mark a function as never-returning : `fn fatal() -> !`.

### Expressions of type `!`

- `panic!("...")` (and the entire panic family)
- `loop { ... }` without `break` (the loop never exits)
- `return expr` (transfers control)
- `break expr` (transfers control out of the loop)
- `continue` (transfers control to the loop head)
- `std::process::exit(n)`
- `unreachable!()`, `todo!()`, `unimplemented!()`

### Edition 2024 fallback change

Before edition 2024 (≤ 2021) : when type inference cannot pin down what `!` should become, it falls back to `()` (unit).

Edition 2024 (Rust 1.85+) : the fallback is `!` itself.

| Construct | 2021 inference | 2024 inference |
|-----------|----------------|----------------|
| `let _ = panic!();` | `T = ()` (fallback) | `T = !` (fallback) |
| `fn f<T: Default>()` called as `f()?` in a generic context | `T = ()` if not constrained | `T = !` -> compile error if `!` does not satisfy bounds (e.g. `Default`) |

Migration : use explicit turbofish (`f::<()>()`) or annotate the local (`let _: () = ...;`). The `never_type_fallback_flowing_into_unsafe` lint is `deny` by default in 2024, it catches the dangerous case where a `!` value crosses into `unsafe` code with assumed `()` semantics.

Source : [edition-guide : never-type-fallback](https://doc.rust-lang.org/edition-guide/rust-2024/never-type-fallback.html).

## 5. ZST and `PhantomData<T>` semantics

Per [std::marker::PhantomData](https://doc.rust-lang.org/std/marker/struct.PhantomData.html) :

- `size_of::<PhantomData<T>>() == 0` for any `T`.
- `align_of::<PhantomData<T>>() == 1`.
- `PhantomData<T>` tells the compiler "this type behaves AS IF it owns a T" for the purposes of variance, drop check, and auto-trait derivation, even when no `T` is actually stored.

### Variance via PhantomData

| Marker form | Variance in `T` |
|-------------|-----------------|
| `PhantomData<T>` | covariant |
| `PhantomData<&T>` | covariant |
| `PhantomData<&mut T>` | invariant |
| `PhantomData<fn(T) -> ()>` | contravariant |
| `PhantomData<fn() -> T>` | covariant |
| `PhantomData<fn(T) -> T>` | invariant |
| `PhantomData<*mut T>` | invariant, opts out of Send/Sync |
| `PhantomData<*const T>` | covariant, opts out of Send/Sync |

### Drop check

A struct holding `PhantomData<T>` is treated as owning `T` for drop-checking. If you need to express "I borrow T but don't drop it" use `PhantomData<&'a T>`. If you need "I will drop T someday" use `PhantomData<T>`.

## 6. Const generics : current stable surface

Stable as of Rust 1.51 (March 2021), expanded through Rust 1.59 :

| Const generic type | Stable? | Example |
|--------------------|---------|---------|
| `usize`, `u8..u128`, `isize`, `i8..i128` | Yes | `<const N: usize>` |
| `bool` | Yes | `<const FLAG: bool>` |
| `char` | Yes | `<const C: char>` |
| Struct / tuple types | No (unstable : `adt_const_params`) | `<const S: MyStruct>` |
| Generic const expressions in bounds | No (unstable : `generic_const_exprs`) | `where [(); N + M]:` |
| `&'static str` | No (unstable) | `<const S: &str>` |
| `f32`, `f64` | No (unstable) | `<const X: f32>` |

For the full mechanics see [[rust-syntax-generics]].
