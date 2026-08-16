# rust-impl-testing: anti-patterns

Seven common testing mistakes seen in real Rust projects, each with a concrete failing pattern, the symptom you will see, the root cause, and the quoted fix.

---

## 1. Putting integration-test helpers in `tests/common.rs`

**Failing pattern**

```
tests/
  common.rs        <-- shared helpers as a flat file
  integration_test.rs
```

```rust
// tests/integration_test.rs
mod common;        // tries to pull common.rs in as a module

#[test]
fn it_works() {
    common::setup();
    assert!(true);
}
```

**Symptom**: Test output contains an extra section with zero tests, e.g.

```
Running tests/common.rs (target/debug/deps/common-1a2b3c)
running 0 tests
```

and (depending on the file's contents) sometimes "unused function `setup`" warnings.

**Root cause**: Cargo compiles every `.rs` file directly inside `tests/` as its own integration-test crate. `tests/common.rs` therefore becomes a second test binary, not a shared module of `tests/integration_test.rs`.

**Fix**: Move the helpers into a subdirectory module. The Rust Book is explicit: "Files in subdirectories of the _tests_ directory don't get compiled as separate crates."

```
tests/
  common/
    mod.rs         <-- shared helpers here
  integration_test.rs
```

```rust
// tests/integration_test.rs
mod common;
common::setup();
```

ALWAYS use `tests/<name>/mod.rs` for shared integration-test code. NEVER put it in `tests/<name>.rs`.

---

## 2. Bare `#[should_panic]` that masks unrelated panics

**Failing pattern**

```rust
#[test]
#[should_panic]
fn rejects_out_of_range() {
    let v: Vec<i32> = vec![];
    let _ = v[42];                 // panic A: index out of bounds
    Guess::new(200);               // panic B: the one we MEANT to assert
}
```

**Symptom**: The test passes even after `Guess::new` is silently changed to no longer panic, because a different bug now panics first.

**Root cause**: Bare `#[should_panic]` accepts ANY panic. Refactors that introduce or remove unrelated panics will not break this test.

**Fix**: Always pin the expected panic message substring.

```rust
#[test]
#[should_panic(expected = "less than or equal to 100")]
fn rejects_out_of_range() {
    Guess::new(200);
}
```

ALWAYS pass `expected = "..."` to `#[should_panic]`. NEVER use the bare form except in trivial one-liner tests where no other panic source exists.

---

## 3. Doc tests that reference private items

**Failing pattern**

```rust
fn validate(input: &str) -> bool { /* private */ true }

/// ```
/// assert!(my_crate::validate("ok"));
/// ```
pub fn process(input: &str) -> bool {
    validate(input)
}
```

**Symptom**: `cargo test --doc` fails with `error[E0603]: function 'validate' is private`.

**Root cause**: A doc test is wrapped in `fn main() { ... }` and gets `extern crate my_crate;` injected. It sees the crate from outside, so it can only call the **public** API. Private items are unreachable, even though they are visible in the source file containing the doc comment.

**Fix**: Either expose the item through the public API, or rewrite the example to use a public entry point. The doc comment lives on a public item, so the example should exercise that public item:

```rust
/// ```
/// assert!(my_crate::process("ok"));
/// ```
```

ALWAYS write doc-test bodies against the public API. NEVER reference private items from a doc test, regardless of which module the comment lives in.

---

## 4. Async test without an `async` runtime attribute

**Failing pattern**

```rust
#[test]
async fn fetches_data() {
    let v = my_service::fetch().await;
    assert_eq!(v, 42);
}
```

**Symptom**: Either a compile error (`async fn` cannot be used as a `#[test]` function on plain libtest) or a runtime panic such as `there is no reactor running, must be called from the context of a Tokio 1.x runtime`.

**Root cause**: `#[test]` does not start any async runtime; the future never gets polled, or the first `.await` on a tokio resource panics because there is no executor.

**Fix**: Use the runtime's own test attribute, which wraps the function in a runtime.

```rust
#[tokio::test]
async fn fetches_data() {
    let v = my_service::fetch().await;
    assert_eq!(v, 42);
}
```

ALWAYS use `#[tokio::test]` (or `#[async_std::test]`, `#[smol_potat::test]`, etc.) for `async fn` tests. NEVER use bare `#[test]` on an async function.

---

## 5. Hidden test interdependence through shared mutable state

**Failing pattern**

```rust
// Each test writes to /tmp/output.txt:
#[test]
fn writes_header() {
    std::fs::write("/tmp/output.txt", "HEADER\n").unwrap();
    // ...
}

#[test]
fn writes_body() {
    let s = std::fs::read_to_string("/tmp/output.txt").unwrap();
    assert!(s.starts_with("HEADER"));
}
```

**Symptom**: Tests pass under `cargo test -- --test-threads=1` but fail intermittently under the default parallel runner; failures are flaky and depend on scheduling.

**Root cause**: `cargo test` runs tests in parallel inside one process. Two `#[test]` functions touching the same file path race.

**Fix**: Either (a) give every test its own temp directory, or (b) serialize. (a) is preferred:

```rust
use tempfile::TempDir;

#[test]
fn writes_header() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("output.txt");
    std::fs::write(&path, "HEADER\n").unwrap();
    // ...
}
```

ALWAYS isolate per-test state through `tempfile::TempDir` or a unique key. NEVER reach for `--test-threads=1` as the first fix; it hides the underlying coupling and slows the entire suite. NEVER mutate global env vars from tests without an explicit serialization mutex.

---

## 6. Using the unstable `#[bench]` attribute on stable

**Failing pattern**

```rust
#![feature(test)]      // does not compile on stable
extern crate test;

#[bench]
fn bench_add(b: &mut test::Bencher) {
    b.iter(|| 2 + 2);
}
```

**Symptom**: On stable Rust: `error[E0658]: use of unstable library feature 'test'`. On nightly it works, but the suite is now permanently locked to a nightly toolchain.

**Root cause**: The built-in `test` crate (and the `#[bench]` attribute it provides) has been unstable since 2015 and there is no plan to stabilize it.

**Fix**: Use `criterion` (or `divan`) under `benches/` with `harness = false`:

`Cargo.toml`:

```toml
[[bench]]
name = "add"
harness = false

[dev-dependencies]
criterion = "0.5"
```

`benches/add.rs`:

```rust
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_add(c: &mut Criterion) {
    c.bench_function("add", |b| b.iter(|| 2 + 2));
}

criterion_group!(benches, bench_add);
criterion_main!(benches);
```

ALWAYS use `criterion` for benchmarks on stable Rust. NEVER use `#[bench]` outside of nightly-only experiments.

---

## 7. Mixing `cargo nextest run` with doc tests in a single CI step

**Failing pattern**

```yaml
# .github/workflows/ci.yml
- run: cargo nextest run --workspace
```

…with `///` doc tests in the codebase and no separate doc-test step.

**Symptom**: Doc tests silently never run in CI. Examples in `/// ... ```` blocks rot until someone notices `cargo test --doc` is broken locally.

**Root cause**: `cargo nextest run` does not execute doc tests. The nextest documentation states this directly: "doctests are not currently supported; run doctests in a separate step with `cargo test --doc`."

**Fix**: Add a sibling step.

```yaml
- run: cargo nextest run --workspace
- run: cargo test --workspace --doc
```

ALWAYS pair `cargo nextest run` with an explicit `cargo test --doc` step in CI. NEVER assume nextest covers doc tests; it does not, and probably will not in the near future.

---

## 8. Combining `#[should_panic]` with a `Result`-returning test

**Failing pattern**

```rust
#[test]
#[should_panic(expected = "boom")]
fn explodes() -> Result<(), Box<dyn std::error::Error>> {
    panic!("boom");
}
```

**Symptom**: Compile error: `the trait bound \`(): Termination\` is not satisfied` or `expected (), found Result<...>` depending on the libtest version, plus a separate error noting that `#[should_panic]` does not apply to `Result`-returning tests.

**Root cause**: libtest treats `-> Result<T, E>` tests and `#[should_panic]` tests as two mutually exclusive failure models: "panicking is success" cannot coexist with "returning `Err` is failure."

**Fix**: Drop the `Result` return type and use a `-> ()` test, or drop `#[should_panic]` and assert `is_err()` instead.

```rust
#[test]
#[should_panic(expected = "boom")]
fn explodes() {
    panic!("boom");
}

// OR

#[test]
fn returns_error() -> Result<(), Box<dyn std::error::Error>> {
    let r: Result<(), &str> = Err("boom");
    assert!(r.is_err());
    Ok(())
}
```

ALWAYS choose one failure model per test. NEVER combine `#[should_panic]` with a `Result` return type.
