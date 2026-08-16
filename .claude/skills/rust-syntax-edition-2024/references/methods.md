# Edition 2024 reference : lints, migration mechanics, semantic detail

This document is the deep reference for edition-2024 migration. It catalogues every lint, every CLI command, and every semantic detail referenced from `SKILL.md`. Reading order suggestion : start with "Migration mechanics" if you are migrating a real crate; start with "Lint catalogue" if you are debugging a specific compiler diagnostic.

## Migration mechanics

### Required toolchain

- `rustc` version 1.85.0 or newer. Earlier toolchains reject `edition = "2024"` in `Cargo.toml`.
- `cargo` from the same toolchain. Verify with `cargo --version`.
- `rustup default stable` (or `rustup default 1.85` for reproducibility).

### Single-crate migration command

```
git status                          # working tree MUST be clean
cargo fix --edition                 # runs every rust_2024_compatibility lint
# inspect diff; commit
git commit -am "Apply cargo fix --edition (2021 -> 2024)"
# bump edition in Cargo.toml
sed -i 's/edition = "2021"/edition = "2024"/' Cargo.toml
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git commit -am "Set edition = \"2024\""
```

### Workspace migration command

For multi-crate workspaces, ALWAYS migrate the crate-graph in topological order : leaves first, then dependents. Cargo does not enforce this, but mixing editions during migration produces confusing diagnostics.

```
cargo fix --edition --workspace
```

After `cargo fix`, manually edit every `Cargo.toml` to set `edition = "2024"`. Set the workspace root `resolver = "3"` explicitly to silence the version-2-vs-3 resolver warning that 1.85 introduces.

### The `rust_2024_compatibility` lint group

This lint group is the umbrella over every individual 2024-incompatibility lint. Add this to a crate root that you intend to migrate later :

```rust
#![warn(rust_2024_compatibility)]
```

The group contains :

- `impl_trait_overcaptures`
- `keyword_idents_2024`
- `missing_unsafe_on_extern`
- `unsafe_attr_outside_unsafe`
- `unsafe_op_in_unsafe_fn` (graduates from `allow` to `warn`)
- `if_let_rescope`
- `tail_expr_drop_order`
- `static_mut_refs`
- `edition_2024_expr_fragment_specifier`
- `rust_2024_prelude_collisions`
- `rust_2024_guarded_string_incompatible_syntax`
- `never_type_fallback_flowing_into_unsafe`

### When `cargo fix --edition` cannot do its job

Two lints lack autofix : `tail_expr_drop_order` and `static_mut_refs`. Two more provide rewrites you may not want : `if_let_rescope` rewrites every flagged `if let` to a `match`, which is verbose; `edition_2024_expr_fragment_specifier` rewrites `expr` to `expr_2021`, which is usually the WRONG choice for forward-compatible macros.

ALWAYS review the migration commit by hand. Treat `cargo fix --edition` as a draft, not a finished change.

## Lint catalogue

Each lint below lists : lint name, level transitions across editions, autofix availability, root-cause summary, and the official URL.

### `impl_trait_overcaptures`

- Editions : `allow` in 2021, `warn`-by-default in 2024 (effectively part of the autofix sweep), pure-syntactic in 2024.
- Autofix : YES. Inserts `+ use<>` (or a specific capture list) to preserve 2021 semantics where the lint detects overcapture.
- Root cause : Rust 2024 changes the implicit-capture rules for return-position `impl Trait`. Pre-2024 only generic parameters appearing in trait bounds were captured by the opaque type. From 2024 onwards, ALL in-scope generic parameters (including lifetimes) are captured.
- Reference : https://doc.rust-lang.org/edition-guide/rust-2024/rpit-lifetime-capture.html

### `never_type_fallback_flowing_into_unsafe`

- Editions : `warn` in 2021, `deny` in 2024.
- Autofix : partial. The lint highlights call sites where the `()`-vs-`!` fallback flows into unsafe code. The fix is annotating the turbofish.
- Root cause : When the never type `!` reaches a generic parameter that is otherwise unconstrained, edition 2021 falls back to `()`; edition 2024 falls back to `!`. The lint is `deny`-by-default because the change can silently flip behaviour of `transmute`-like calls that previously inferred `()`.
- Reference : https://doc.rust-lang.org/edition-guide/rust-2024/never-type-fallback.html

### `missing_unsafe_on_extern`

- Editions : `allow` in 2021, `warn`-by-default in 2024 (autofix sweep).
- Autofix : YES. Prefixes every `extern "ABI" { ... }` with `unsafe`.
- Root cause : edition 2024 requires `unsafe extern "ABI" { ... }` to make the contract explicit : the author of the block asserts that the declared signatures match the foreign symbol's actual ABI. The block contents may individually declare `safe fn ...` for items whose preconditions are universally satisfied.
- Reference : https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-extern.html

### `unsafe_attr_outside_unsafe`

- Editions : `allow` in 2021, `warn`-by-default in 2024.
- Autofix : YES. Rewrites `#[no_mangle]` to `#[unsafe(no_mangle)]`, `#[link_section = "..."]` to `#[unsafe(link_section = "...")]`, `#[export_name = "..."]` to `#[unsafe(export_name = "...")]`.
- Root cause : These three attributes influence symbol naming and linking; misuse causes symbol collisions and undefined behaviour at link time. Wrapping them in `unsafe(...)` makes the human responsibility visible.
- Reference : https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-attributes.html

### `unsafe_op_in_unsafe_fn`

- Editions : `allow` in 2021 (opt-in), `warn`-by-default in 2024.
- Autofix : YES. Wraps unsafe ops in inner `unsafe { ... }` blocks.
- Root cause : `unsafe fn foo()` historically conflated "this function REQUIRES unsafe to CALL" with "this function's body MAY USE unsafe ops freely". Edition 2024 separates these. Callers still need an outer `unsafe { ... }` to invoke the function; the body must itself open inner `unsafe { ... }` blocks for individual unsafe ops, so reviewers can audit each op.
- Reference : https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-op-in-unsafe-fn.html

### `if_let_rescope`

- Editions : `allow` in 2021, `warn`-by-default in 2024.
- Autofix : YES (rewrites to `match`, which preserves 2021 drop scope). ALWAYS review : the `match` rewrite is verbose and frequently NOT what you want; the better fix is to accept the new 2024 scope (which fixes deadlock bugs).
- Root cause : The scrutinee of `if let pattern = expr { ... } else { ... }` is now dropped at the end of the `if let` arm or before entering `else`, not at the end of the surrounding scope. This is the change that "fixes" the classic RwLock-read-then-write deadlock.
- Reference : https://doc.rust-lang.org/edition-guide/rust-2024/temporary-if-let-scope.html

### `tail_expr_drop_order`

- Editions : `allow` in 2021, `warn`-by-default in 2024.
- Autofix : NO. Manual review is mandatory.
- Root cause : Temporaries created in a block's tail expression are now dropped BEFORE the block's local variables. Pre-2024 they were dropped after. Code that relies on a tail-expression temporary borrowing a local will now fail to compile (`E0597 does not live long enough`). The lint flags potential problems but cannot automatically rewrite the offending expression because the fix is context-dependent (introduce a `let`-binding, restructure the expression, or accept the diagnostic).
- Reference : https://doc.rust-lang.org/edition-guide/rust-2024/temporary-tail-expr-scope.html

### `static_mut_refs`

- Editions : `warn` in 2021, `deny`-by-default in 2024.
- Autofix : NO. There is no semantically-equivalent rewrite that preserves intent.
- Root cause : Taking a shared or mutable reference to a `static mut` violates Rust's aliasing-XOR-mutation invariant the instant the reference exists, even if it is never used. This is instant undefined behaviour. Migration target is atomics, `Mutex`/`RwLock`, `OnceLock`/`LazyLock`, or `&raw const`/`&raw mut` raw pointers when reference semantics are not actually needed.
- Reference : https://doc.rust-lang.org/edition-guide/rust-2024/static-mut-references.html

### `edition_2024_expr_fragment_specifier`

- Editions : autofix in 2024.
- Autofix : YES (rewrites `expr` to `expr_2021`); usually you want to KEEP `expr` to support `const { ... }` and `_`.
- Root cause : The `expr` macro fragment specifier was conservatively defined when `const` blocks (Rust 1.79) and `_` expressions (Rust 1.59) shipped; widening `expr` in those releases would have been a breaking change to macros with overlapping rules. Edition 2024 widens `expr`. A new specifier `expr_2021` preserves the strict pre-2024 grammar for macros that need it.
- Reference : https://doc.rust-lang.org/edition-guide/rust-2024/macro-fragment-specifiers.html

### `keyword_idents_2024`

- Editions : `allow` in 2021, `warn`-by-default in 2024.
- Autofix : YES (escapes `gen` identifiers as `r#gen`).
- Root cause : `gen` is reserved as a keyword for forthcoming `gen { ... }` generator blocks (RFC 3513). The block syntax itself is NOT stable on edition 2024; only the reservation is. User identifiers named `gen` must be renamed or escaped.
- Reference : https://doc.rust-lang.org/edition-guide/rust-2024/gen-keyword.html

### `rust_2024_prelude_collisions`

- Editions : `warn`-by-default in 2024.
- Autofix : YES (rewrites ambiguous method calls to fully-qualified syntax).
- Root cause : `std::future::Future` and `std::future::IntoFuture` are now in the prelude. User-defined traits with methods named `poll` or `into_future` previously resolved unambiguously; in 2024 the compiler sees both the prelude trait and the user trait in scope, and errors with `E0034 multiple applicable items in scope`. The autofix rewrites the call site to `<Receiver as UserTrait>::poll(...)`.
- Reference : https://doc.rust-lang.org/edition-guide/rust-2024/prelude.html

### `rust_2024_guarded_string_incompatible_syntax`

- Editions : `warn`-by-default in 2024.
- Autofix : YES.
- Root cause : Reserves new syntax for guarded string literals (a forthcoming RFC). The lint detects sequences that would change meaning under the new rule. Encountered rarely in practice.

## Semantic detail : RPIT lifetime capture

The pre-2024 capture rules were defined by RFC 1522 ("Conservative impl trait") and refined in RFC 2515. The 2024 default is RFC 3617's "edition-2024 capture rules": opaque-type capture sets include every generic parameter (lifetime or type) in scope at the impl-Trait position. `use<'a, T>` is the explicit override syntax stabilized in 1.82.

The `+ use<>` empty-capture set is meaningful and parseable; it is the recommended migration shorthand for opaque types that genuinely need to be `'static`.

When the RPIT appears inside a trait definition (RPITIT, since 1.75), the 2024 capture rules apply identically. Precise capturing inside trait definitions stabilized in Rust 1.87 (May 2025).

## Semantic detail : never-type fallback

The fallback rule applies when type inference reaches an unconstrained variable that is unified with `!`. The fallback inserts a default type to "fix" the variable. The classic case is `?` propagation inside a generic function : an unused `Result<T, E>::Err` arm could leave `T` unconstrained, and the compiler then needs SOME type for `T`.

Edition 2021 chose `()` because in practice almost all such variables came from `try`-style code paths where `()` was the user's intent. Edition 2024 chose `!` because `()` allowed unsoundness in `transmute<T, U>(...)` call sites where the inference-driven `T = ()` and the user's expected `T = !` were observably different. The `deny`-by-default level of `never_type_fallback_flowing_into_unsafe` exists for exactly that case.

## Semantic detail : `unsafe extern` and `safe fn`

Pre-2024 `extern "C" { ... }` blocks declared bindings whose ABI the compiler trusts the author about. Every call site needed `unsafe { ... }` regardless of the function's actual preconditions.

Edition 2024 inverts the model. The BLOCK is `unsafe`, asserting that the SIGNATURES are correct. Items declared `safe fn ...` inside the block may be called WITHOUT `unsafe { ... }` at the call site, because the author has asserted that the function's preconditions are unconditionally satisfied (typical example : `libc::abort`, which has no preconditions). Items declared `unsafe fn ...` or with no modifier default to requiring `unsafe { ... }` at the call site.

## Semantic detail : drop-scope changes (`if_let_rescope` and `tail_expr_drop_order`)

The drop-order changes are independent in spec but related in practice; both reduce the lifetime of temporaries to match programmer intuition more closely.

`if_let_rescope` : the SCRUTINEE temporary's drop scope is the `if let` ARM, not the surrounding statement. Pattern :

```
{
    let lock = mtx.lock().unwrap();        // outer local
    if let Some(x) = compute(&*lock) {     // SCRUTINEE temporary
        // 2021 + 2024: scrutinee still alive here
    } else {
        // 2021: scrutinee still alive (deadlock if `compute` returned a guard)
        // 2024: scrutinee already dropped here
    }
    // 2021: scrutinee dropped here
    // 2024: scrutinee already dropped
}
```

`tail_expr_drop_order` : temporaries in a block's TAIL expression now drop BEFORE the block's locals, not after :

```
fn f() -> usize {
    let c = RefCell::new("..");
    c.borrow().len()      // tail: `c.borrow()` returns Ref<'_, _>
                          // 2021: temporary dropped after `c` ; OK
                          // 2024: temporary dropped before `c` ; E0597
}
```

The 2024 order matches the order of `let`-bindings : last-declared dies first, tail-expression temporaries die before any block-local `let`s.

## Cargo defaults shift in 1.85

- `cargo new` defaults to `edition = "2024"` in the generated `Cargo.toml`.
- `cargo new` defaults to `resolver = "3"` (workspace-aware MSRV-aware resolver introduced in 1.84).
- `rust-version` workspace inheritance is strengthened : a member crate inheriting `workspace = true` from the workspace `[package]` table now correctly inherits the workspace `rust-version`.

## When to skip migration

ALWAYS migrate eventually. Edition support is a long-term commitment from the Rust project, but new compiler features (precise capturing in trait definitions, the eventual `gen` blocks, future syntax) ship gated on edition. The longer you wait, the larger the migration commit.

EXCEPT : do NOT migrate if any of the following hold :

- You depend on a crate that mass-uses `static mut` references and you cannot fork-and-fix.
- Your crate's MSRV declares < 1.85.
- Your `cargo fix --edition` output requires manual `tail_expr_drop_order` review that exceeds your review bandwidth this sprint.

In those cases, add `#![warn(rust_2024_compatibility)]` to flag every incompatibility as soon as the toolchain supports it, and migrate later.
