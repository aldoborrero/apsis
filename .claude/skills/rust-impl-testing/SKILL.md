---
name: rust-impl-testing
description: >
  Use when the user writes unit tests (`#[test]`), integration tests in `tests/`, doc tests, async tests, criterion benches, uses `cargo nextest`, or controls panicking tests via `#[should_panic]`.
  Prevents mixing integration tests in same crate, doc tests that compile only against stale signatures, async tests without runtime, and unguarded panicking tests.
  Covers: `#[test]` unit tests, `#[cfg(test)]` module convention, integration tests in `tests/` (each file is a separate crate), doc tests in /// comments, `cargo test --doc`, `#[should_panic]` / `#[should_panic(expected = "...")]`, `#[ignore]` for slow tests, `cargo nextest run`, property testing crates (proptest, quickcheck), snapshot testing (insta), criterion benches, common test helpers via `tests/common/mod.rs`.
  Keywords: test, "#[test]", "cargo test", "#[cfg(test)]", "tests directory", integration test, doc test, "cargo test --doc", "should_panic", ignore, "cargo test --ignored", "cargo nextest", nextest, criterion, benchmark, proptest, quickcheck, insta, "snapshot test", "test fixture", "tests/common", "test helpers", "async test", "tokio::test".
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# rust-impl-testing

The **Rust testing** skill: how to write unit tests, integration tests, and doc tests; how Cargo's three test kinds map to file layout; how to use `#[should_panic]`, `#[ignore]`, and `Result<(), E>`-returning tests; how to run async tests with `#[tokio::test]`; when to switch from `cargo test` to `cargo nextest run`; and how to add property, snapshot, and benchmark testing on stable Rust.

Cross-references: [[rust-impl-cargo-project]] (the `[profile.test]` profile, `[dev-dependencies]`, the `test` and `bench` fields), [[rust-impl-async-tokio]] (`#[tokio::test]` runtime selection), [[rust-syntax-async-await]] (calling `.await` in test bodies).

---

## When to use this skill

- User writes `#[test]` functions, or wraps them in `#[cfg(test)] mod tests { ... }`.
- User asks where integration tests go (`src/`, `tests/`, `examples/`?).
- User writes doc tests in `///` comments and one of them stops compiling.
- User wants negative tests via `#[should_panic]` or wants to skip slow tests via `#[ignore]`.
- User writes an `async fn` test and gets a runtime panic.
- User wants a faster runner than `cargo test`, or per-test process isolation.
- User wants property-based, snapshot, or micro-benchmark coverage on stable Rust.

For Cargo's project layout, `[dev-dependencies]`, and the `[profile.test]` / `[profile.bench]` profiles, see [[rust-impl-cargo-project]]. For `tokio` runtime details under `#[tokio::test]`, see [[rust-impl-async-tokio]].

---

## Decision tree: which kind of test belongs where

```
What do you want to test?
   A private function or module internals
      -> Unit test in src/, under `#[cfg(test)] mod tests { use super::*; ... }`

   The crate's public API as an external consumer would use it
      -> Integration test in tests/<name>.rs (each file is its own crate)

   The exact code shown in a doc comment compiles and runs
      -> Doc test in /// comments inside the item it documents
         Only available for `lib` crates; for `bin` crates, move logic to lib.rs

   Behaviour that depends on async code (.await)
      -> #[tokio::test] (or async-std equivalent), not bare #[test]

   A panic is the success condition (precondition violations, etc.)
      -> #[should_panic(expected = "substring of panic message")]

   A statistical / many-input check
      -> proptest! { ... } or quickcheck

   Pretty-printed regression of a value
      -> insta::assert_snapshot! / assert_yaml_snapshot!

   Micro-benchmark on stable
      -> criterion in benches/, NOT the unstable #[bench] attribute
```

ALWAYS keep unit tests in the same file as the code they test, gated by `#[cfg(test)]`. NEVER place unit tests for module internals inside `tests/`: `tests/` only has access to the public API.

---

## Anatomy of a unit test

The convention is one `tests` submodule per source file:

```rust
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_two_positives() {
        assert_eq!(add(2, 2), 4);
    }
}
```

Rules:

- ALWAYS gate the test module with `#[cfg(test)]` so production builds skip compilation, dependencies, and symbols.
- ALWAYS `use super::*;` so the test module sees private items of the parent module.
- The test runner discovers any function annotated with `#[test]`; the function MUST take no arguments.

Assertion macros (`assert!`, `assert_eq!`, `assert_ne!`) accept an optional format-string message:

```rust
#[test]
fn greeting_contains_name() {
    let result = greeting("Carol");
    assert!(
        result.contains("Carol"),
        "Greeting did not contain name, value was `{result}`"
    );
}
```

For tests that should return errors instead of panicking, return `Result<(), E>` and use `?`:

```rust
#[test]
fn parses_input() -> Result<(), std::num::ParseIntError> {
    let n: i32 = "42".parse()?;
    assert_eq!(n, 42);
    Ok(())
}
```

NEVER combine `Result<T, E>` return with `#[should_panic]`: the two are mutually exclusive. To assert an `Err` value, use `assert!(value.is_err());` in a normal `-> ()` test.

---

## Anatomy of an integration test

Integration tests live in the top-level `tests/` directory next to `src/`. Each `.rs` file is compiled as **its own crate**, with the crate-under-test brought in via `use mycrate::...`.

```
my_crate/
  Cargo.toml
  src/
    lib.rs
  tests/
    integration_test.rs
    common/
      mod.rs
```

```rust
// tests/integration_test.rs
use my_crate::add_two;

mod common;

#[test]
fn it_adds_two() {
    common::setup();
    assert_eq!(add_two(2), 4);
}
```

Rules:

- ALWAYS put shared helpers in `tests/common/mod.rs`, NEVER in `tests/common.rs`. A bare `tests/common.rs` is compiled as a separate integration-test binary and shows up in the test summary as an empty `running 0 tests` line.
- ALWAYS access the crate-under-test through its public API. There is no `super::*` available; the integration test sits outside the library.
- For binary crates (`src/main.rs` only), integration tests cannot call into the binary directly. ALWAYS move logic into `src/lib.rs` and let `main.rs` be a thin wrapper, then integration-test the library.

---

## Doc tests

Code in triple-backtick blocks inside `///` comments is compiled and run by `cargo test --doc`. Rustdoc wraps each block in an implicit `fn main()` and an implicit `extern crate <yourcrate>;` (unless `#![doc(test(no_crate_inject))]` is set), so simple examples need no boilerplate:

```rust
/// Adds two to its argument.
///
/// # Examples
///
/// ```
/// let n = my_crate::add_two(3);
/// assert_eq!(n, 5);
/// ```
pub fn add_two(x: i32) -> i32 { x + 2 }
```

Hide setup with `#`-prefixed lines (they are compiled and executed, but not rendered in the docs):

```rust
/// ```
/// # use my_crate::*;
/// let n = add_two(3);
/// assert_eq!(n, 5);
/// ```
```

Doc-test code-fence info-strings control behaviour:

| Info-string         | Effect                                                                   |
|---------------------|--------------------------------------------------------------------------|
| (none) / `rust`     | Compile and run the example.                                              |
| `ignore`            | Skip both compilation and execution. Last resort; prefer the others.      |
| `no_run`            | Compile but do not run (good for networked or never-terminating code).    |
| `should_panic`      | The block MUST panic at runtime to pass.                                  |
| `compile_fail`      | The block MUST fail to compile to pass.                                   |
| `edition2024`       | Compile this block under the named edition.                               |

Use `?` inside doc tests by hiding a `main` that returns a `Result`:

```rust
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let n: i32 = "42".parse()?;
/// assert_eq!(n, 42);
/// # Ok(()) }
/// ```
```

Doc tests run only for library targets. ALWAYS test binary logic through a library crate; doc tests on `src/main.rs` are not executed.

---

## `#[should_panic]` and `#[ignore]`

`#[should_panic]` inverts success: the test passes if (and only if) the body panics. Without an `expected = "..."` substring it matches **any** panic, including unrelated bugs that happened to fire first.

```rust
#[test]
#[should_panic(expected = "less than or equal to 100")]
fn rejects_out_of_range() {
    Guess::new(200);
}
```

ALWAYS pass `expected = "..."` to `#[should_panic]` so the test still fails when an unrelated panic masks the one you meant to catch. NEVER combine `#[should_panic]` with `-> Result<(), E>`.

`#[ignore]` skips a test by default (typically slow or environment-dependent):

```rust
#[test]
#[ignore]
fn expensive_integration() { /* ... */ }
```

Invocations:

- `cargo test` runs everything except ignored tests.
- `cargo test -- --ignored` runs **only** the ignored tests.
- `cargo test -- --include-ignored` runs **all** tests, ignored or not.

---

## Running tests: the cargo CLI surface

The synopsis is `cargo test [options] [testname] [-- test-options]`. Arguments before `--` go to Cargo (target selection); arguments after `--` go to the libtest harness (filtering, threads, output).

| Goal                                  | Command                                            |
|---------------------------------------|----------------------------------------------------|
| Run all tests (lib + bin + integration + doc) | `cargo test`                                      |
| Only library unit tests               | `cargo test --lib`                                 |
| Only integration tests                | `cargo test --tests`                               |
| One specific integration-test file    | `cargo test --test integration_test`               |
| Only doc tests                        | `cargo test --doc`                                 |
| Filter by name substring              | `cargo test parse`                                 |
| Show stdout of passing tests          | `cargo test -- --show-output`                      |
| Single-threaded execution             | `cargo test -- --test-threads=1`                   |
| Run ignored tests only                | `cargo test -- --ignored`                          |
| Run ignored + non-ignored             | `cargo test -- --include-ignored`                  |

ALWAYS use `--test-threads=1` when tests share mutable global state (env vars, current dir, on-disk files in a fixed path). Better: redesign the test to use a `tempfile::TempDir` so parallelism is safe.

---

## Async tests

A plain `#[test]` does not start a runtime; an `async fn` test will fail to compile or panic at first `.await`. Use the runtime's test attribute instead.

```rust
#[tokio::test]
async fn fetches_data() {
    let value = my_service::fetch().await.unwrap();
    assert_eq!(value, 42);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fetches_data_multithreaded() { /* ... */ }
```

ALWAYS depend on `tokio = { version = "1", features = ["macros", "rt"] }` (or `rt-multi-thread` for the multi-threaded flavor) in `[dev-dependencies]`. See [[rust-impl-async-tokio]] for runtime details.

---

## `cargo nextest`

`cargo nextest` is a faster, process-isolated test runner. Each test runs in its own subprocess, which catches leaked global state and produces clearer failure reports. It does NOT run doc tests; ALWAYS pair it with a separate `cargo test --doc` step in CI.

```bash
# Install (Linux/macOS pre-built binary into ~/.cargo/bin):
curl -LsSf https://get.nexte.st/latest/linux | tar zxf - -C ${CARGO_HOME:-~/.cargo}/bin
# Or:
cargo binstall cargo-nextest --secure

# Run:
cargo nextest run                 # all (non-doc) tests
cargo nextest run --workspace     # every member crate
cargo nextest run -E 'test(parse)'   # filterset DSL
```

Per-profile configuration lives in `.config/nextest.toml`. For a typical CI step, configure `retries = 2` on flaky integration tests and keep doc tests in a separate `cargo test --doc` job.

---

## Property, snapshot, and benchmark testing

See `references/methods.md` for the full lookup tables. Quick map:

- **`proptest`** (`proptest!` macro, strategies, shrinking) and **`quickcheck`** (`#[quickcheck]`): generate inputs automatically and shrink failing cases. ALWAYS pin a seed file or `PROPTEST_CASES` count for reproducible CI.
- **`insta`** (`insta::assert_snapshot!`, `assert_yaml_snapshot!`): records the printed form of a value to a `.snap` file; review changes with `cargo insta review`. ALWAYS check `.snap` files into version control.
- **`criterion`**: stable-Rust micro-benchmark framework. Put benches under `benches/`, declare them in `Cargo.toml` as `[[bench]] name = "..." harness = false`. NEVER use the built-in `#[bench]` attribute: it is permanently unstable and only available on nightly with `#![feature(test)]`.

---

## Common test layout for a library crate

```
my_crate/
  Cargo.toml             # [dev-dependencies] for tokio/proptest/insta/criterion
  src/
    lib.rs               # pub items, with #[cfg(test)] mod tests in each file
    parser.rs
  tests/
    api.rs               # public-API integration test
    common/
      mod.rs             # shared helpers, NOT tests/common.rs
  benches/
    parse.rs             # criterion bench, harness = false
```

ALWAYS keep this shape; tooling, IDE integrations, and `cargo` defaults assume it.

---

## See also

- `references/methods.md`: full table of `cargo test` / `cargo nextest` flags, doc-test attributes, and dev-dependency setup snippets.
- `references/examples.md`: end-to-end example crates for unit + integration + doc + async + property + snapshot + criterion bench.
- `references/anti-patterns.md`: the seven anti-patterns this skill prevents, with quoted fix-its.
- [[rust-impl-cargo-project]] for `[dev-dependencies]`, `[[test]]`, `[[bench]]`, and `[profile.test]`.
- [[rust-impl-async-tokio]] for `#[tokio::test]` flavors and runtime configuration.
- [[rust-syntax-async-await]] for `.await` semantics in test bodies.

Sources (verified 2026-05-19):

- The Rust Book, ch. 11 "Writing Automated Tests": https://doc.rust-lang.org/book/ch11-00-testing.html
- rustdoc, "Documentation tests": https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html
- The Cargo Book, `cargo test`: https://doc.rust-lang.org/cargo/commands/cargo-test.html
- cargo-nextest: https://nexte.st/
