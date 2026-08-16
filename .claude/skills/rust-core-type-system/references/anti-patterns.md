# Type System : Anti-Patterns

Six common mistakes when working with Rust's type system, each with a root-cause analysis and the correct alternative. Sources : Rust Reference (type-layout, never type), edition-guide (never-type-fallback), real-world bug reports.

## AP-1 : Taking a reference to a `#[repr(packed)]` field

### The mistake

```rust
#[repr(C, packed)]
struct PacketHeader {
    version: u8,
    length: u16,  // misaligned : offset 1, but u16 wants alignment 2
}

fn parse(buf: &[u8; 3]) -> u16 {
    let header = unsafe { &*(buf.as_ptr() as *const PacketHeader) };
    let len_ref: &u16 = &header.length;  // UNDEFINED BEHAVIOR
    *len_ref
}
```

### Why it fails

Creating a reference to a misaligned field violates Rust's reference-alignment invariant : every `&T` must point to a properly aligned T. On x86/x64 you may "get away with it" (silent corruption); on ARM/MIPS it segfaults. The compiler is allowed to assume aligned access and may emit SIMD or aligned-load instructions on the reference. Source : Rust Reference, type-layout.

The Rust compiler now refuses to derive `Debug`, `Hash`, `PartialEq`, etc. on `#[repr(packed)]` structs because the derived code would internally take field references. As of Rust 1.82, you can use `&raw const` / `&raw mut` syntax for raw pointer creation without going through a reference.

### Fix

```rust
// Option A : copy the field out to a local.
fn parse(buf: &[u8; 3]) -> u16 {
    let header = unsafe { &*(buf.as_ptr() as *const PacketHeader) };
    let length = header.length;  // copy is fine, no reference taken
    length.to_be()
}

// Option B : raw pointer + unaligned read (Rust 1.82+).
fn parse_b(buf: &[u8; 3]) -> u16 {
    let header = unsafe { &*(buf.as_ptr() as *const PacketHeader) };
    let ptr: *const u16 = &raw const header.length;
    unsafe { ptr.read_unaligned() }
}
```

## AP-2 : Relying on field order in default repr

### The mistake

```rust
struct Header {
    magic: u32,
    flags: u8,
    version: u8,
    payload_size: u16,
}

fn dump_to_disk(h: &Header) {
    // Assumes magic is at offset 0, flags at 4, version at 5, payload_size at 6.
    let bytes = unsafe {
        std::slice::from_raw_parts(h as *const _ as *const u8, std::mem::size_of::<Header>())
    };
    std::fs::write("header.bin", bytes).unwrap();
}
```

### Why it fails

Default `#[repr(Rust)]` makes NO ordering guarantee. The compiler may pack fields in any order (typically by alignment, descending) to minimize padding. The on-disk layout becomes a compiler-version-dependent format. Source : Rust Reference, type-layout : "There are no guarantees of data layout made by this representation."

### Fix

Add `#[repr(C)]` when bytes are exported, imported, or shared between processes/threads/compilers :

```rust
#[repr(C)]
struct Header {
    magic: u32,
    flags: u8,
    version: u8,
    payload_size: u16,
}
```

## AP-3 : Assuming `Option<u32>` is 4 bytes

### The mistake

```rust
struct EntityId(Option<u32>);

fn main() {
    assert_eq!(std::mem::size_of::<EntityId>(), 4);  // PANIC : it is 8
}
```

### Why it fails

`u32` has 2^32 valid bit patterns, no niche. The compiler must add a discriminant byte to encode `None`, and with alignment padding the total grows to 8 bytes (4 for u32 + 1 discriminant + 3 padding). `Option<u32>` is twice the size of `u32`.

### Fix

Use `NonZeroU32` when 0 is invalid for your ID space :

```rust
use std::num::NonZeroU32;
struct EntityId(Option<NonZeroU32>);

fn main() {
    assert_eq!(std::mem::size_of::<EntityId>(), 4);
    // None encoded as bit pattern 0 (invalid for NonZeroU32).
}
```

The `std` docs guarantee : `size_of::<Option<NonZeroU32>>() == size_of::<u32>()`. Source : `std::num::NonZeroU32`.

## AP-4 : Assuming structural typing

### The mistake

```rust
struct UserId(u64);
struct PostId(u64);

fn fetch_post(id: u64) -> Post { /* ... */ }

let uid = UserId(42);
fetch_post(uid);  // ERROR : expected u64, found UserId
fetch_post(uid.0);  // Compiles, but you just passed a USER ID to fetch_post !
```

### Why it fails

Rust is strictly nominally typed. `UserId` and `u64` are distinct types even though `UserId` is a one-field newtype. This is a feature, not a bug, but developers coming from TypeScript or Go (structural typing) often try `as` casts or `.0` extraction to "make it work", defeating the safety. The bug at the bottom (`fetch_post(uid.0)`) compiles but is semantically wrong : a user ID passed to a function expecting a post ID.

### Fix

Either change `fetch_post` to accept `PostId`, or convert explicitly with named functions :

```rust
fn fetch_post(id: PostId) -> Post { /* ... */ }

let pid = PostId(42);
fetch_post(pid);  // type-checked
```

If you genuinely need a cross-type conversion, make it explicit and named (`fn user_to_post_id(u: UserId) -> PostId`). NEVER use `.0` extraction at call sites to bypass the type system.

## AP-5 : Edition-2024 never-type fallback breakage

### The mistake

This compiles on edition 2021 but FAILS on edition 2024 :

```rust
fn process<T>(x: T) -> Result<T, ()> {
    fn helper<U: Default>() -> Result<U, ()> {
        Ok(U::default())
    }
    helper()?;  // unconstrained U
    Ok(x)
}
```

### Why it fails

On edition 2021, the unconstrained `U` falls back to `()` (which implements `Default`), so the call compiles. On edition 2024, `U` falls back to `!` (the never type), and `!` does not implement `Default`, so the call fails. Worse : code that compiled cleanly may produce subtly different runtime behaviour if it relied on `()` semantics in inference-driven branches.

Per edition-guide : "Never type (`!`) to any type ('never-to-any') coercions fall back to never type (`!`) rather than to unit type (`()`)." The `never_type_fallback_flowing_into_unsafe` lint is `deny` by default to catch the dangerous case where `!` flows into `unsafe` code that assumed `()`.

### Fix

Annotate the inference site explicitly :

```rust
fn process<T>(x: T) -> Result<T, ()> {
    fn helper<U: Default>() -> Result<U, ()> {
        Ok(U::default())
    }
    helper::<()>()?;  // explicit turbofish
    Ok(x)
}
```

Or use a let-binding with type annotation : `let () = helper()?;`. ALWAYS audit inference points (especially `?` chains in generic contexts) when upgrading to edition 2024.

## AP-6 : Const generic expressions in stable code

### The mistake

```rust
struct Buffer<const N: usize> {
    data: [u8; N],
}

impl<const N: usize> Buffer<N> {
    // ERROR on stable : generic_const_exprs is unstable
    fn doubled(self) -> Buffer<{ N * 2 }> {
        unimplemented!()
    }
}
```

### Why it fails

Stable Rust (as of 1.85) permits const generics with primitive integer types, `bool`, and `char` as PARAMETERS, but const expressions in type positions (`{ N * 2 }`) require the unstable `generic_const_exprs` feature. The compiler rejects the example because it cannot prove the expression is well-formed for all instantiations on stable.

### Fix

For now, take the doubled size as a separate const generic parameter and provide a constructor that asserts the relationship :

```rust
struct Buffer<const N: usize> {
    data: [u8; N],
}

impl<const N: usize> Buffer<N> {
    fn doubled<const M: usize>(self) -> Buffer<M> {
        assert_eq!(M, N * 2);  // runtime check
        unimplemented!()
    }
}

// Caller : let b2: Buffer<8> = Buffer::<4>::new().doubled::<8>();
```

Or wait for stabilization of `generic_const_exprs`. The `typenum` crate provides an alternative (type-level numbers) at the cost of compile-time complexity. For the full const-generic landscape see [[rust-syntax-generics]].
