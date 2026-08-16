# rust-impl-testing: examples reference

End-to-end, copy-pasteable test code for each kind of Rust test. Every example compiles under `rustc 1.85` with `edition = "2024"`.

---

## 1. Minimal library crate with unit tests

`Cargo.toml`:

```toml
[package]
name = "my_crate"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[lib]
path = "src/lib.rs"
```

`src/lib.rs`:

```rust
//! `my_crate` adds numbers.

/// Adds two integers.
///
/// # Examples
///
/// ```
/// assert_eq!(my_crate::add(2, 3), 5);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    inner_add(a, b)
}

fn inner_add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_two_positives() {
        assert_eq!(add(2, 2), 4);
    }

    #[test]
    fn private_fn_visible_from_tests() {
        // `inner_add` is private but reachable from the same module's
        // child `tests` module via `use super::*;`.
        assert_eq!(inner_add(2, 3), 5);
    }

    #[test]
    fn assert_with_message() {
        let result = add(2, 2);
        assert_eq!(result, 4, "expected 4, got {result}");
    }
}
```

Run:

```bash
cargo test            # unit tests + doc test
cargo test --lib      # unit tests only
cargo test --doc      # doc test only
```

---

## 2. Integration tests with shared helpers

Directory layout:

```
my_crate/
  Cargo.toml
  src/
    lib.rs
  tests/
    api.rs
    common/
      mod.rs
```

`tests/common/mod.rs`:

```rust
//! Shared helpers for integration tests. Lives in tests/common/mod.rs
//! (NOT tests/common.rs) so it is NOT compiled as a separate integration
//! test binary.

use std::sync::Once;

static INIT: Once = Once::new();

pub fn setup() {
    INIT.call_once(|| {
        // one-shot setup (logging, env, fixtures, ...)
    });
}
```

`tests/api.rs`:

```rust
use my_crate::add;

mod common;

#[test]
fn adds_via_public_api() {
    common::setup();
    assert_eq!(add(2, 3), 5);
}
```

Run only this integration file:

```bash
cargo test --test api
```

---

## 3. `#[should_panic]` and `#[ignore]`

```rust
pub struct Guess(i32);

impl Guess {
    pub fn new(value: i32) -> Self {
        if !(1..=100).contains(&value) {
            panic!("Guess value must be less than or equal to 100, got {value}.");
        }
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "less than or equal to 100")]
    fn rejects_out_of_range() {
        Guess::new(200);
    }

    #[test]
    #[ignore]
    fn expensive_full_sweep() {
        for n in 1..=100 {
            let _ = Guess::new(n);
        }
    }
}
```

Run:

```bash
cargo test                              # skips expensive_full_sweep
cargo test -- --ignored                 # runs ONLY expensive_full_sweep
cargo test -- --include-ignored         # runs everything
```

---

## 4. `Result`-returning tests with `?`

```rust
#[test]
fn parses_then_doubles() -> Result<(), std::num::ParseIntError> {
    let n: i32 = "21".parse()?;
    assert_eq!(n * 2, 42);
    Ok(())
}
```

The trailing `Ok(())` returns success. NEVER add `#[should_panic]` to a `Result`-returning test; they are mutually exclusive.

---

## 5. Async tests with `#[tokio::test]`

`Cargo.toml`:

```toml
[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

`src/lib.rs`:

```rust
pub async fn fetch() -> u32 { 42 }

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn current_thread_runtime() {
        assert_eq!(fetch().await, 42);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn multi_thread_runtime() {
        assert_eq!(fetch().await, 42);
    }

    // Time-paused runtime is useful for testing timeouts/intervals.
    #[tokio::test(start_paused = true)]
    async fn time_paused() {
        let start = tokio::time::Instant::now();
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        // With start_paused = true, sleep returns immediately and the clock
        // jumps; real wall-clock time barely moved.
        assert!(start.elapsed() < std::time::Duration::from_millis(100));
    }
}
```

See [[rust-impl-async-tokio]] for the full runtime feature matrix.

---

## 6. Doc tests with hidden setup and `?`

```rust
/// Parses a decimal integer.
///
/// # Examples
///
/// ```
/// # use my_crate::parse_int;
/// let n = parse_int("42").unwrap();
/// assert_eq!(n, 42);
/// ```
///
/// Failing inputs return an error:
///
/// ```
/// # use my_crate::parse_int;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// assert!(parse_int("not-a-number").is_err());
/// # Ok(()) }
/// ```
///
/// A snippet that intentionally panics:
///
/// ```should_panic
/// my_crate::parse_int("nope").unwrap();
/// ```
pub fn parse_int(s: &str) -> Result<i32, std::num::ParseIntError> {
    s.parse()
}
```

Run:

```bash
cargo test --doc
cargo test --doc -- --show-output
```

---

## 7. Property test with `proptest`

`Cargo.toml`:

```toml
[dev-dependencies]
proptest = "1"
```

`src/lib.rs`:

```rust
pub fn abs_diff(a: i32, b: i32) -> u32 {
    a.abs_diff(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn abs_diff_is_symmetric(a in any::<i32>(), b in any::<i32>()) {
            prop_assert_eq!(abs_diff(a, b), abs_diff(b, a));
        }

        #[test]
        fn abs_diff_zero_iff_equal(a in any::<i32>()) {
            prop_assert_eq!(abs_diff(a, a), 0);
        }
    }
}
```

Run with a reproducible seed by checking the auto-generated `proptest-regressions/` directory into git, or pin the case count:

```bash
PROPTEST_CASES=1024 cargo test
```

---

## 8. Snapshot test with `insta`

`Cargo.toml`:

```toml
[dev-dependencies]
insta = "1"
```

`src/lib.rs`:

```rust
pub fn report(items: &[&str]) -> String {
    let mut out = String::from("Report\n======\n");
    for (i, item) in items.iter().enumerate() {
        out.push_str(&format!("{i}. {item}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_snapshot() {
        let s = report(&["alpha", "beta", "gamma"]);
        insta::assert_snapshot!(s);
    }
}
```

First run writes `src/snapshots/my_crate__tests__report_snapshot.snap.new`. Review and accept:

```bash
cargo install cargo-insta
cargo insta review
```

ALWAYS commit `.snap` files; they ARE the test oracle.

---

## 9. Criterion benchmark on stable

`Cargo.toml`:

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "add"
harness = false
```

`benches/add.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use my_crate::add;

fn bench_add(c: &mut Criterion) {
    c.bench_function("add 2+2", |b| {
        b.iter(|| add(black_box(2), black_box(2)))
    });
}

criterion_group!(benches, bench_add);
criterion_main!(benches);
```

Run:

```bash
cargo bench --bench add
# Reports in target/criterion/report/index.html
```

NEVER use the built-in `#[bench]` attribute on stable: it is gated on nightly behind `#![feature(test)]` and has been unstable since 2015.

---

## 10. CI step combining nextest + doc tests

```bash
# Fast, parallel, process-isolated; does NOT cover doc tests:
cargo nextest run --workspace --profile ci

# Cover doc tests in a separate step:
cargo test --workspace --doc
```

`.config/nextest.toml` excerpt:

```toml
[profile.ci]
retries = 2
slow-timeout = { period = "30s", terminate-after = 3 }
fail-fast = false
```
