# Edition 2024 reference : anti-patterns and recovery

This document catalogues the migration mistakes that recur in real Rust 2024 migrations. Each anti-pattern lists the symptom, the underlying mistake, the fix, and the long-term prevention strategy. Sources : Rust GitHub issues, the rust-lang internals forum, the 1.85 release thread.

## Anti-pattern 1 : skipping `cargo fix --edition`

### Symptom

Developer edits `Cargo.toml` to set `edition = "2024"` directly, then attempts to `cargo build`. The compiler emits dozens of diagnostics scattered across the crate, many with confusing error messages mixing `impl_trait_overcaptures`, `unsafe_op_in_unsafe_fn`, and unrelated downstream errors.

### Mistake

Manually setting the edition skips the `cargo fix --edition` driver. The driver runs every individual 2024-compatibility lint with the autofix-emitting machine, in the correct order, on a tree already verified to compile under edition 2021. Without that orderly pass, the developer sees lint diagnostics interleaved with cascading errors from un-autofixed code.

### Fix

```
git checkout -- Cargo.toml          # revert edition bump
cargo fix --edition                  # let the driver do its job
git diff                             # review autofixes
# THEN edit Cargo.toml
```

### Prevention

ALWAYS use `cargo fix --edition` as the FIRST step. NEVER edit `edition = "..."` by hand before the autofix sweep. The driver is the supported migration path; manual editing is undefined behaviour from a tooling perspective.

## Anti-pattern 2 : running `cargo fix --edition --allow-dirty` on uncommitted work

### Symptom

The developer's changes are mixed with `cargo fix --edition`'s autofixes in the same diff. Reviewing the migration commit becomes impossible because the diff conflates developer-intent edits with mechanical lint fixes.

### Mistake

`--allow-dirty` bypasses Cargo's safety check that the working tree is clean. The check exists exactly to ensure migration diffs are reviewable.

### Fix

Commit (or stash) developer work first, then run `cargo fix --edition` on a clean tree :

```
git stash
cargo fix --edition
git commit -am "Apply cargo fix --edition (2021 -> 2024)"
git stash pop
```

### Prevention

NEVER pass `--allow-dirty`. The flag is documented as an emergency override; treat it as such.

## Anti-pattern 3 : ignoring `rust_2024_compatibility` warnings on a 2021 crate

### Symptom

The crate stays on edition 2021 for months. When the developer eventually runs `cargo fix --edition`, the autofix touches hundreds of files because every new line written in the interim added more 2024-incompatible idioms.

### Mistake

The `rust_2024_compatibility` lint group exists specifically to flag future incompatibilities during the 2021 phase. Disabling it (or never enabling it) lets technical debt accumulate.

### Fix

Add the lint group to every crate root immediately :

```rust
#![warn(rust_2024_compatibility)]
```

If the lint group surfaces warnings, fix them incrementally as part of regular development. Migration day then becomes a no-op.

### Prevention

Standard CI policy : every Rust crate, regardless of current edition, MUST enable `#![warn(rust_2024_compatibility)]` or its successor.

## Anti-pattern 4 : adding `unsafe` to `extern` blocks by hand in pre-2024 codebases

### Symptom

A developer reads the edition-2024 migration guide and "gets a head start" by manually rewriting `extern "C" { ... }` to `unsafe extern "C" { ... }` in a crate still on edition 2021. The compiler errors :

```
error[E0658]: `unsafe extern { ... }` blocks are only stable in edition 2024
```

### Mistake

The `unsafe extern { ... }` syntax is gated on edition 2024. The same is NOT true for the `unsafe(...)` attribute wrappers, which ARE stable independent of edition (since 1.82). Conflating the two leads to incorrect head-start attempts.

### Fix

Roll back the `extern` block changes. Keep the `unsafe(...)` attribute changes :

```rust
// Keep this on any edition (1.82+)
#[unsafe(no_mangle)]
pub extern "C" fn entry() {}

// Revert this on edition 2021
extern "C" {                // not "unsafe extern" until edition 2024
    pub fn external_thing();
}
```

### Prevention

When in doubt about edition vs version gating, consult the Rust Edition Guide chapter for the change. The guide separates "edition-gated" syntax from "version-stable since X" syntax.

## Anti-pattern 5 : `cargo fix --edition` autofix accepted without review

### Symptom

After migration, runtime behaviour changes silently. The most common case : an `if let pattern = expr { ... } else { ... }` was auto-rewritten to `match`, preserving 2021 deadlock-prone scope, when the developer's intent was actually to FIX the deadlock by adopting 2024 scope.

### Mistake

Treating `cargo fix --edition` as a fire-and-forget tool. The autofix exists to preserve compilability; it does NOT necessarily preserve the developer's intent for any given change.

### Fix

After every `cargo fix --edition` run, review each touched file. For `if_let_rescope` rewrites specifically :

- If the `if let` was acquiring a lock in the scrutinee and the `else` arm needed a different lock, REVERT the auto-rewrite to the `if let` form (with 2024 scope, the deadlock disappears).
- If the `if let` scrutinee was a `RefCell::borrow()` and the `else` arm needed `borrow_mut()`, REVERT the auto-rewrite for the same reason.
- If the `else` arm did not interact with the scrutinee resource at all, KEEP the `match` rewrite (no behavioural difference).

### Prevention

Document the autofix policy in the team's Rust style guide. Make `if_let_rescope`-rewrite review a mandatory step in every 2024-migration PR template.

## Anti-pattern 6 : leaving `#[no_mangle]` without the `unsafe(...)` wrapper after migration

### Symptom

After `cargo fix --edition` and a manual review, the developer notices `#[no_mangle]` still on some functions and "fixes" them by hand by deleting the attribute, breaking the FFI surface.

### Mistake

Two compounding errors : (1) the developer assumes `cargo fix --edition` missed the attribute, when in fact the lint may have been suppressed by an earlier `#[allow(...)]`; (2) the developer's reaction is destructive (deletion) instead of corrective (wrapping).

### Fix

Restore the attribute with the `unsafe(...)` wrapper :

```rust
// SAFETY: this is the only definition of the symbol `entry` in the link target.
#[unsafe(no_mangle)]
pub extern "C" fn entry() { /* ... */ }
```

If multiple call sites and the developer is unsure which functions need it : `git log -p` the deleted attributes from the migration commit to find them.

### Prevention

NEVER delete attributes during migration. Wrap or keep. The "delete and see what breaks" strategy is incompatible with FFI exports because the breakage is at link time, not compile time.

## Anti-pattern 7 : writing new `pub fn foo() -> impl Trait` in 2024 without thinking about capture

### Symptom

A function `pub fn cached<'a>(key: &'a str) -> impl Iterator<Item = u8>` compiles fine, but downstream callers cannot store the returned iterator past the lifetime of `key`. Pre-2024 they could.

### Mistake

The developer is writing new code on edition 2024 with pre-2024 mental model. The 2024 RPIT default captures all in-scope lifetimes, including `'a`, so the returned `impl Iterator` is bounded by `'a`.

### Fix

Decide explicitly :

- If the returned iterator GENUINELY borrows from `key`, ACCEPT the 2024 default. The compiler's behaviour matches reality.
- If the returned iterator does NOT borrow from `key` (e.g. it clones the key into a `String` internally), opt out with `+ use<>` :

```rust
pub fn cached<'a>(key: &'a str) -> impl Iterator<Item = u8> + use<> {
    let owned = key.to_string();
    owned.into_bytes().into_iter()
}
```

### Prevention

When designing new APIs on edition 2024, write down the capture set EXPLICITLY in the signature. Either accept the 2024 default and document why, or write `+ use<...>` and document why.

## Anti-pattern 8 : assuming `unsafe_op_in_unsafe_fn` is just lint noise

### Symptom

A developer disables `unsafe_op_in_unsafe_fn` crate-wide with `#![allow(unsafe_op_in_unsafe_fn)]` because the autofix added "too many" inner `unsafe { ... }` blocks.

### Mistake

The lint exists to make EACH unsafe operation auditable in isolation. Suppressing it crate-wide reintroduces the conflation between "calling this fn requires unsafe" and "the body of this fn may freely perform unsafe ops". Code reviewers can no longer trust that every flagged `unsafe { ... }` block represents a verified safety contract.

### Fix

Re-enable the lint :

```rust
// NEVER write this on edition 2024:
// #![allow(unsafe_op_in_unsafe_fn)]
```

Then write inner `unsafe { ... }` blocks and `// SAFETY:` comments individually. The keystroke cost is real but small; the audit value is large.

### Prevention

Treat `unsafe_op_in_unsafe_fn` as a hard requirement, not a stylistic preference. Add `#![deny(unsafe_op_in_unsafe_fn)]` to crates with substantial unsafe surfaces.

## Anti-pattern 9 : refactoring `static mut` to `unsafe { Box::leak(...) }` to "preserve performance"

### Symptom

After the `static_mut_refs` lint denies a reference to a `static mut COUNTER: i32`, the developer rewrites the static as :

```rust
use std::sync::OnceLock;
static COUNTER: OnceLock<*mut i32> = OnceLock::new();

fn init() {
    let _ = COUNTER.set(Box::leak(Box::new(0)) as *mut i32);
}
```

claiming the original `static mut COUNTER` was "just as unsafe".

### Mistake

This rewrite is strictly MORE unsafe than the original. The raw pointer stored in `OnceLock` has no synchronisation; concurrent reads and writes are undefined behaviour exactly as before. The `OnceLock` wrapper synchronises ONLY the initialisation, not the subsequent mutations.

### Fix

Use an atomic for primitive integers :

```rust
use std::sync::atomic::{AtomicI32, Ordering};
static COUNTER: AtomicI32 = AtomicI32::new(0);
COUNTER.fetch_add(1, Ordering::Relaxed);
```

Or a `Mutex` for non-primitive state :

```rust
use std::sync::Mutex;
static STATE: Mutex<Vec<String>> = Mutex::new(Vec::new());
STATE.lock().unwrap().push("hello".into());
```

For one-time lazy initialisation, `OnceLock<T>` (1.70+) or `LazyLock<T>` (1.80+) :

```rust
use std::sync::LazyLock;
static CONFIG: LazyLock<String> = LazyLock::new(|| load_config());
```

### Prevention

The `static_mut_refs` lint is `deny`-by-default in 2024 for a reason : the underlying pattern is unsound. ALWAYS migrate to a synchronised primitive. NEVER "preserve the unsafety" with a thin pointer wrapper.

## Anti-pattern 10 : autoring "edition 2024 makes my code slower" without measurement

### Symptom

After migration, the developer notices `cargo test` takes longer or a microbenchmark regresses. They blame the edition itself.

### Mistake

Edition 2024 is a SOURCE-language migration. The compiler's code generation is unchanged by the edition flag. Any performance change is attributable to one of :

- The `tail_expr_drop_order` change : drops happen at different points, which can change `Drop` impl ordering. Real impact only on hot loops with non-trivial destructors.
- The `if_let_rescope` change : locks release earlier, which can actually IMPROVE throughput by reducing contention windows.
- New stabilised library APIs the developer started using (unrelated to the edition).
- Background `cargo` work (newer rustc may have different default optimisation pipelines).

### Fix

Benchmark with `cargo bench` BEFORE and AFTER the edition migration commit, using the same rustc version. If a regression is real and reproducible, file the case against the specific change (almost always `tail_expr_drop_order` for destructor-heavy code).

### Prevention

Treat edition migration as a source-level refactor. Measure performance separately, with a controlled rustc version, and attribute changes to specific stabilised features rather than to the edition itself.

## Anti-pattern 11 : leaving `#[allow(unsafe_op_in_unsafe_fn)]` on an `unsafe fn` in a public API

### Symptom

A library publishes an API with a function like :

```rust
#[allow(unsafe_op_in_unsafe_fn)]
pub unsafe fn freed_ptr<T>(_: *mut T) {
    /* unsafe ops without inner blocks */
}
```

Downstream users see the function as audited (it's in a public API), but the lint suppression hides which ops are unsafe.

### Mistake

Public-API unsafety must be MAXIMALLY auditable. Suppressing the lint on a public function makes downstream review impossible.

### Fix

Remove the `#[allow(...)]` ; add inner `unsafe { ... }` blocks with `// SAFETY:` comments :

```rust
pub unsafe fn freed_ptr<T>(p: *mut T) {
    // SAFETY: caller guarantees `p` was obtained from a previous Box::into_raw.
    unsafe { drop(Box::from_raw(p)) };
}
```

### Prevention

Treat lint suppression in public APIs as a code-review red flag. NEVER suppress safety-relevant lints on `pub unsafe fn` signatures.

## Anti-pattern 12 : conflating `expr_2021` autofix with intent

### Symptom

`cargo fix --edition` rewrites every `expr` fragment in declarative macros to `expr_2021`. The developer commits the change, then later cannot use `const { ... }` in callers because the macro rejects it.

### Mistake

The autofix is CONSERVATIVE : it preserves pre-2024 grammar semantics by rewriting `expr` to `expr_2021`. For most macros, the developer actually WANTS the wider `expr` grammar that accepts `const { ... }` and `_`.

### Fix

Review every `expr_2021` rewrite. Revert to `expr` unless the macro has overlapping rules (e.g. `($e:expr) =>` AND `(const $e:expr) =>` in the same macro_rules block) :

```rust
macro_rules! demo {
    ($e:expr) => { /* matches const { ... } too in 2024 ; usually desired */ };
}

macro_rules! demo_strict {
    ($e:expr_2021) => { /* genuinely needs pre-2024 grammar */ };
    (const $e:expr_2021) => { /* second rule that would conflict */ };
}
```

### Prevention

ALWAYS review `edition_2024_expr_fragment_specifier` rewrites. The "right" answer depends on macro design intent, which only the human author knows.
