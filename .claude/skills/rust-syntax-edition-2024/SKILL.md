---
name: rust-syntax-edition-2024
description: >
  Use when the user migrates a crate from edition 2021 to 2024, writes new edition-2024 code, hits the never-type fallback semantic change, encounters `unsafe extern` requirement, or wonders why RPIT lifetime capture default changed.
  Prevents writing pre-2024 idioms in 2024 code, missing the `cargo fix --edition` migration step, ignoring the `unsafe_op_in_unsafe_fn` lint becoming warn-by-default, or running into the `tail_expr_drop_order` change silently.
  Covers: all 13 edition-2024 changes shipped in Rust 1.85 (RPIT lifetime capture, never-type fallback, `unsafe extern`, `unsafe(no_mangle)` and `unsafe(link_section)`, `expr` fragment widening, prelude additions, `unsafe_op_in_unsafe_fn` warn-by-default, `if let` chains scope, `tail_expr_drop_order`, gen blocks status, `rust_2024_compatibility` lint group, reserved syntax), migration workflow.
  Keywords: edition 2024, "cargo fix --edition", "rust 2024", "1.85", "edition migration", "never type fallback", "! to ()", "unsafe extern", "unsafe(no_mangle)", "RPIT capture", "use<>", "unsafe_op_in_unsafe_fn", "if let chain", "tail_expr_drop_order", "expr fragment", "gen block", prelude 2024, "rust_2024_compatibility", "edition guide", "what changed in 2024", "should I migrate".
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# rust-syntax-edition-2024

Edition 2024 stabilized in Rust 1.85.0 (2025-02-20). It is the largest edition the Rust project has shipped: 13 language changes, library prelude additions, Cargo default shifts (`cargo new` defaults to `edition = "2024"` and resolver `"3"`), and Rustfmt rule updates. This skill is the authoritative reference every other syntax/impl skill in this package depends on. ALWAYS treat edition 2024 as the baseline; NEVER ship code in this package using pre-2024 idioms unless explicitly demonstrating a migration delta.

## Quick Reference

### Core rules (deterministic)

1. ALWAYS migrate via `cargo fix --edition` on a clean working tree. NEVER hand-edit edition idioms before running the automated migration; you will miss lints that only the rustc driver emits.
2. NEVER write `extern "C" { ... }` in edition 2024 code. ALWAYS write `unsafe extern "C" { ... }`; items inside are `unsafe` by default and may be opted into `safe` per item.
3. NEVER use bare `#[no_mangle]`, `#[link_section = "..."]`, or `#[export_name = "..."]` in edition 2024. ALWAYS wrap them as `#[unsafe(no_mangle)]`, `#[unsafe(link_section = "...")]`, `#[unsafe(export_name = "...")]`.
4. ALWAYS wrap every unsafe operation inside an `unsafe fn` body in its own `unsafe { ... }` block. The 2024 default is `#[warn(unsafe_op_in_unsafe_fn)]`; pre-2024 code that calls unsafe ops bare inside an unsafe fn body breaks the lint.
5. NEVER assume RPIT (`-> impl Trait`) does NOT capture an input lifetime in edition 2024. The 2024 default captures all in-scope generics and lifetimes. Opt out with `+ use<>` (or list specific params with `+ use<'a, T>`).
6. NEVER rely on `!` falling back to `()` in 2024. The fallback is `!` itself; type-inference outcomes for `?`-and-`unwrap`-only paths change. Annotate the turbofish (`f::<()>()`) at the call site when migration breaks.
7. NEVER take a shared or mutable reference to a `static mut`. The `static_mut_refs` lint is `deny`-by-default in 2024. ALWAYS migrate to `AtomicX`, `Mutex<T>`, `OnceLock<T>`, `LazyLock<T>`, or `&raw const`/`&raw mut`.
8. NEVER use `gen` as an identifier in edition 2024 code. ALWAYS rename, or escape as `r#gen` if you cannot. `gen` blocks themselves remain unstable.
9. ALWAYS bind the result of a `RefCell::borrow()` / `Mutex::lock()` to a `let` before the tail expression dot-chains it. The `tail_expr_drop_order` change drops temporaries before locals in 2024.
10. ALWAYS rewrite `if let Some(x) = read_lock() { ... } else { write_lock() ... }` patterns. In 2024 the scrutinee temporary drops BEFORE entering `else`, which fixes deadlocks but changes drop order observably.

### Decision tree: "Should I migrate this crate to 2024?"

```
Does the crate compile cleanly on Rust 1.85 or newer with edition 2021?
  NO  -> Fix 1.85 compile errors first, THEN migrate edition.
  YES -> Run `cargo +stable fix --edition` on clean working tree.
    Did `cargo fix --edition` apply changes without error?
      YES -> Bump `edition = "2024"` in Cargo.toml. Build + test.
        Tests pass?
          YES -> Commit. Run `cargo clippy` for new lints. Done.
          NO  -> Inspect `never_type_fallback`, `tail_expr_drop_order`,
                 `if_let_rescope` lints in build log. Apply targeted fixes.
      NO  -> Read failure output. Most common cause is references to
             `static mut` (lint `static_mut_refs`) which has NO autofix.
             Refactor to atomics / Mutex / OnceLock, then re-run.
```

### Migration checklist (mandatory order)

1. `git status` must be clean. `cargo build` must be green on edition 2021 with rustc 1.85+.
2. `cargo fix --edition` (NOT `cargo fix --edition --allow-dirty` unless you committed your `cargo fix` output deliberately).
3. Review diff. The driver runs every lint in the `rust_2024_compatibility` lint group.
4. Edit `Cargo.toml`: `edition = "2024"`. Optionally set `resolver = "3"` if you have a workspace root.
5. `cargo build`; `cargo test`; `cargo clippy --all-targets`.
6. Manually inspect any `tail_expr_drop_order` warnings (no autofix); migrate or accept.
7. Manually inspect any `static_mut_refs` errors (no autofix); refactor away from `static mut`.
8. Commit migration as a single dedicated commit.

### The 13 language changes (cheat-sheet table)

| # | Change | Autofix? | Lint name |
|---|--------|----------|-----------|
| 1 | RPIT captures all in-scope lifetimes | YES | `impl_trait_overcaptures` |
| 2 | Never-type fallback `()` -> `!` | partial | `never_type_fallback_flowing_into_unsafe` |
| 3 | `unsafe extern { ... }` mandatory | YES | `missing_unsafe_on_extern` |
| 4 | `#[unsafe(no_mangle)]` etc. required | YES | `unsafe_attr_outside_unsafe` |
| 5 | `unsafe_op_in_unsafe_fn` warn-by-default | YES | `unsafe_op_in_unsafe_fn` |
| 6 | `if let` scrutinee dropped before `else` | YES (rewrite to `match`) | `if_let_rescope` |
| 7 | Tail-expression temporaries drop before locals | NO | `tail_expr_drop_order` |
| 8 | `static mut` refs `deny`-by-default | NO | `static_mut_refs` |
| 9 | `expr` fragment matches `const {}` and `_` | YES (rewrite to `expr_2021`) | `edition_2024_expr_fragment_specifier` |
| 10 | `gen` reserved keyword | YES (`r#gen`) | `keyword_idents_2024` |
| 11 | Reserved guarded string literal syntax | YES | `rust_2024_guarded_string_incompatible_syntax` |
| 12 | Match-ergonomics restrictions for `&` patterns | YES | `mismatched_lifetime_syntaxes` (closely related) |
| 13 | Prelude adds `Future`, `IntoFuture` | YES (FQN rewrite) | `rust_2024_prelude_collisions` |

ALL of the above except items 2 (partial), 7, and 8 are fully covered by `cargo fix --edition`. Items 7 and 8 ALWAYS require human review.

## Patterns

### Pattern: RPIT lifetime capture (item 1)

In edition 2024, every in-scope lifetime and type parameter is implicitly captured by `-> impl Trait`. Pre-2024 only types/lifetimes that syntactically appeared in trait bounds were captured.

Side-by-side:

```rust
// edition 2021
fn f<'a>(x: &'a [u8]) -> impl Sized { 42_u32 }
// `'a` NOT captured. `impl Sized` is `'static`-compatible.

// edition 2024 (same source)
fn f<'a>(x: &'a [u8]) -> impl Sized { 42_u32 }
// `'a` IS captured. The returned opaque type is bounded by `'a`.

// edition 2024, restoring 2021 semantics:
fn f<'a>(x: &'a [u8]) -> impl Sized + use<> { 42_u32 }
//                                  ^^^^^^^ explicit "capture nothing"

// edition 2024, capturing a subset:
fn g<'a, 'b, T>(x: &'a [u8], y: &'b T) -> impl Sized + use<'a, T> { 42_u32 }
//                                                     ^^^^^^^^^^ explicit list
```

When the caller requires `'static` from the returned opaque type, 2024 overcapture is a compile error; add `+ use<>` to opt out. The `impl_trait_overcaptures` lint flags every such case; `cargo fix --edition` inserts `+ use<>` automatically. See [[rust-syntax-lifetimes]] for the broader use<> story.

### Pattern: never-type fallback (item 2)

```rust
// edition 2021: `T` falls back to `()` when otherwise unconstrained.
fn outer<T>(x: T) -> Result<T, ()> {
    fn f<T: Default>() -> Result<T, ()> { Ok(T::default()) }
    f()?;          // T = ()
    Ok(x)
}

// edition 2024: `T` falls back to `!`. `!: Default` does NOT hold, so error.
// Fix: annotate the turbofish.
fn outer<T>(x: T) -> Result<T, ()> {
    fn f<T: Default>() -> Result<T, ()> { Ok(T::default()) }
    f::<()>()?;    // explicit
    Ok(x)
}
```

The `never_type_fallback_flowing_into_unsafe` lint is `deny`-by-default in 2024 because the change interacts with `unsafe` code paths in dangerous ways. See [references/methods.md](references/methods.md) for the safety rationale.

### Pattern: unsafe extern (item 3)

```rust
// edition 2021
extern "C" {
    pub fn sqrt(x: f64) -> f64;                        // implicitly unsafe to call
    pub unsafe fn strlen(p: *const std::ffi::c_char) -> usize;
}

// edition 2024
unsafe extern "C" {
    pub safe fn sqrt(x: f64) -> f64;                   // callable WITHOUT unsafe{}
    pub unsafe fn strlen(p: *const std::ffi::c_char) -> usize;
    pub fn free(p: *mut core::ffi::c_void);            // defaults to unsafe
}
```

The `safe` modifier per item is a 2024 feature: it lets you declare bindings whose ABI you have audited and whose preconditions hold unconditionally. See [[rust-impl-ffi-bindgen]].

### Pattern: unsafe attributes (item 4)

```rust
// edition 2021
#[no_mangle]
pub extern "C" fn example() {}

#[link_section = ".init_array"]
static INIT: extern "C" fn() = example;

// edition 2024
// SAFETY: no other global function exports this symbol name.
#[unsafe(no_mangle)]
pub extern "C" fn example() {}

#[unsafe(link_section = ".init_array")]
static INIT: extern "C" fn() = example;
```

ALWAYS write a `// SAFETY:` comment above each `#[unsafe(...)]` attribute. The compiler does not enforce it, but reviewers will reject changes without it.

### Pattern: unsafe_op_in_unsafe_fn (item 5)

```rust
// edition 2021 (warns only if you opt-in to the lint)
unsafe fn read_unchecked<T>(slice: &[T], i: usize) -> &T {
    slice.get_unchecked(i)         // unsafe op, no inner block needed
}

// edition 2024 (warns by default)
unsafe fn read_unchecked<T>(slice: &[T], i: usize) -> &T {
    // SAFETY: caller guarantees `i < slice.len()`.
    unsafe { slice.get_unchecked(i) }
}
```

See [[rust-syntax-unsafe]] for the broader two-purposes-of-`unsafe-fn` discussion.

### Pattern: if let rescope (item 6) and tail expr drop order (item 7)

```rust
use std::sync::RwLock;

// edition 2021: read lock held through the `else` block -> deadlock when
// `value.write()` is called.
fn f_2021(value: &RwLock<Option<bool>>) {
    if let Some(x) = *value.read().unwrap() {
        println!("{x}");
    } else {
        let mut w = value.write().unwrap();   // DEADLOCK in 2021
        if w.is_none() { *w = Some(true); }
    }
}

// edition 2024: read lock dropped before entering `else`. No deadlock.
// Same source compiles; behaviour changes.
```

Tail-expression drop order pairs with this. A `RefCell::borrow()` chained off a local goes wrong in 2024 because the borrow temporary now drops BEFORE the local:

```rust
use std::cell::RefCell;

// edition 2021: compiles. Temporary outlives `c`.
// edition 2024: error E0597 "`c` does not live long enough".
fn len() -> usize {
    let c = RefCell::new("..");
    c.borrow().len()
}

// edition 2024 fix:
fn len_2024() -> usize {
    let c = RefCell::new("..");
    let borrowed = c.borrow();
    borrowed.len()
}
```

Items 6 and 7 are the two most common 2024-migration footguns. The `if_let_rescope` lint suggests rewriting as `match` (preserves 2021 semantics); the `tail_expr_drop_order` lint flags potential issues but provides NO autofix.

### Pattern: static mut references (item 8)

```rust
// edition 2021 (warns)
static mut COUNTER: i32 = 0;
unsafe { let _r = &COUNTER; }            // shared ref to static mut

// edition 2024 (`deny`-by-default; hard error in practice)
// Migrate to one of:
use std::sync::atomic::{AtomicI32, Ordering};
static COUNTER: AtomicI32 = AtomicI32::new(0);
COUNTER.fetch_add(1, Ordering::Relaxed);

// Or, when raw pointer access is genuinely required:
static mut COUNTER: i32 = 0;
let p = &raw const COUNTER;              // 1.82+ syntax, NO reference taken
```

There is NO automatic migration for `static_mut_refs`. ALWAYS refactor toward atomics, `Mutex`, `OnceLock`, or `&raw const`/`&raw mut`. See [[rust-syntax-unsafe]] for strict-provenance APIs.

### Pattern: expr fragment widening (item 9), gen keyword (item 10), prelude additions (item 13)

```rust
// item 9: `expr` now matches `const { ... }` and `_`. Use `expr_2021` for the
// strict pre-2024 grammar inside macros that already match `const $e:expr`.
macro_rules! takes_either {
    ($e:expr) => { /* matches now also const blocks and _ */ };
    (const $e:expr_2021) => { /* preserves pre-2024 grammar */ };
}

// item 10: `gen` is reserved. Rename identifiers OR escape with r#:
fn r#gen() -> i32 { 0 }

// item 13: Future / IntoFuture are now in the prelude.
// If you defined a trait with a `poll` method previously, you may now see
// ambiguity errors. `cargo fix --edition` rewrites the call site to FQN:
//     <_ as MyTrait>::poll(&pinned)
```

See [[rust-syntax-pattern-matching]] for the related match-ergonomics tightening (item 12).

## Edge Cases

- The `unsafe(...)` attribute syntax (item 4) is independently stable since Rust 1.82, so you can adopt it BEFORE migrating the edition. Doing so reduces churn in the migration commit.
- The precise-capturing `+ use<...>` syntax (item 1) is stable since Rust 1.82 as well; the EDITION change is the implicit-capture default, not the syntax availability.
- `cargo new` defaults to `edition = "2024"` AND `resolver = "3"` since 1.85. If you scaffold subprojects inside a workspace whose root uses `resolver = "2"`, Cargo warns and uses the workspace setting; declare `resolver` explicitly at the workspace root to avoid surprises. See [[rust-impl-cargo-project]].
- The `rust_2024_compatibility` lint group is a UMBRELLA group: enabling it during 2021 development surfaces every 2024-incompatibility before you migrate. ALWAYS add `#![warn(rust_2024_compatibility)]` to crate roots that target a future migration.
- Edition is a per-CRATE property, NOT a per-workspace property. A workspace MAY mix 2021 and 2024 crates. Public APIs across the boundary are stable; only the source-level idioms differ.

## Cross-References

- [[rust-syntax-lifetimes]] : RPIT lifetime capture deep dive, `use<>` semantics, variance.
- [[rust-syntax-unsafe]] : `unsafe_op_in_unsafe_fn` rationale, 8 unsafe superpowers, strict-provenance APIs replacing `static mut` refs.
- [[rust-syntax-pattern-matching]] : match-ergonomics changes for `&` patterns (item 12).
- [[rust-core-type-system]] : never-type `!` and its fallback rules.
- [[rust-impl-ffi-bindgen]] : `unsafe extern` blocks and the `safe` per-item modifier.
- [[rust-core-language-versions]] : Rust 1.80 through 1.87 release-by-release feature index; edition 2024 is the 1.85 entry.

## Reference Links

- Migration mechanics, lint table, semantic detail per change: [references/methods.md](references/methods.md)
- Full working before/after snippets for all 13 items: [references/examples.md](references/examples.md)
- Real-world migration footguns and how to recover: [references/anti-patterns.md](references/anti-patterns.md)

## Official Sources

- Rust Edition Guide : Rust 2024 chapter index : https://doc.rust-lang.org/edition-guide/rust-2024/index.html
- Edition 2024 RPIT lifetime capture : https://doc.rust-lang.org/edition-guide/rust-2024/rpit-lifetime-capture.html
- Edition 2024 never-type fallback : https://doc.rust-lang.org/edition-guide/rust-2024/never-type-fallback.html
- Edition 2024 unsafe extern : https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-extern.html
- Edition 2024 unsafe attributes : https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-attributes.html
- Edition 2024 unsafe_op_in_unsafe_fn : https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-op-in-unsafe-fn.html
- Edition 2024 if let rescope : https://doc.rust-lang.org/edition-guide/rust-2024/temporary-if-let-scope.html
- Edition 2024 tail-expr drop order : https://doc.rust-lang.org/edition-guide/rust-2024/temporary-tail-expr-scope.html
- Edition 2024 static mut references : https://doc.rust-lang.org/edition-guide/rust-2024/static-mut-references.html
- Edition 2024 macro fragment specifiers : https://doc.rust-lang.org/edition-guide/rust-2024/macro-fragment-specifiers.html
- Edition 2024 gen keyword : https://doc.rust-lang.org/edition-guide/rust-2024/gen-keyword.html
- Edition 2024 prelude additions : https://doc.rust-lang.org/edition-guide/rust-2024/prelude.html
- Rust 1.85.0 release announcement : https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/
