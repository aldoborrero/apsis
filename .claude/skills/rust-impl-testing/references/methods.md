# rust-impl-testing: methods reference

Authoritative lookup tables for `cargo test`, `cargo nextest`, doc-test info-strings, and the standard `[dev-dependencies]` you need to wire up unit, integration, doc, async, property, snapshot, and benchmark testing on stable Rust.

All commands verified against `rustc 1.85` + `cargo 1.85` (edition 2024). Sources at end of file.

---

## 1. Test attributes (built-in)

| Attribute                                | Where it goes              | Effect                                                                                |
|------------------------------------------|----------------------------|----------------------------------------------------------------------------------------|
| `#[test]`                                | On a `fn` taking no args   | Marks the function as a test; the test harness picks it up.                            |
| `#[cfg(test)]`                           | On a `mod` (or item)       | Item is compiled only when the test profile is active (`cargo test`).                  |
| `#[should_panic]`                        | On a `#[test]` fn          | Test passes only if the body panics.                                                   |
| `#[should_panic(expected = "...")]`      | On a `#[test]` fn          | Test passes only if the body panics AND the message contains the substring.            |
| `#[ignore]`                              | On a `#[test]` fn          | Test is skipped by default; runs only under `--ignored` or `--include-ignored`.        |
| `#[bench]` (nightly only)                | On a `fn(&mut Bencher)`    | Reserved for libtest's unstable bench harness; on stable use `criterion` instead.      |

ALWAYS prefer `#[should_panic(expected = "...")]` over bare `#[should_panic]`; the substring guard catches unrelated panics that would otherwise pass silently.

---

## 2. `cargo test` flag reference

Synopsis: `cargo test [options] [testname] [-- test-binary-options]`. The `--` separator marks the boundary between Cargo's own flags and the libtest harness's flags.

### Target selection (before `--`)

| Flag                      | Meaning                                                                                 |
|---------------------------|------------------------------------------------------------------------------------------|
| `--lib`                   | Build and run only the package's library tests.                                          |
| `--bins`                  | Build and run tests for every binary target.                                             |
| `--bin <NAME>`            | Build and run tests for one named binary.                                                |
| `--tests`                 | Build and run every target with `test = true` (library + binaries + integration tests).  |
| `--test <NAME>`           | Build and run one named integration test (a single file in `tests/`).                    |
| `--examples`              | Build and run every example target.                                                      |
| `--example <NAME>`        | Build and run one named example.                                                         |
| `--benches`               | Build and run every target with `bench = true`.                                          |
| `--bench <NAME>`          | Build and run one named benchmark.                                                       |
| `--all-targets`           | Equivalent to `--lib --bins --tests --benches --examples`.                               |
| `--doc`                   | Run only doc tests (and ONLY doc tests; conflicts with the other selectors above).       |
| `--workspace`             | Test every package in the workspace.                                                     |
| `--no-run`                | Compile but do not execute tests (useful for CI artifacts).                              |
| `--no-fail-fast`          | Continue running tests after a failure; report all results.                              |

### libtest harness flags (after `--`)

| Flag                       | Meaning                                                                                |
|----------------------------|-----------------------------------------------------------------------------------------|
| `--test-threads=<N>`       | Run at most `N` tests in parallel (`1` = serial).                                       |
| `--show-output`            | Print stdout/stderr of passing tests as well (failures always print).                   |
| `--nocapture`              | Disable output capture entirely (use sparingly: interleaved output).                    |
| `--include-ignored`        | Run all tests including ignored ones.                                                   |
| `--ignored`                | Run only ignored tests.                                                                 |
| `<substring>`              | Positional filter: run tests whose full path contains this substring.                   |
| `--exact <name>`           | Match the test name exactly, not as substring.                                          |
| `--list`                   | List discovered tests instead of running them.                                          |
| `--format=pretty|terse|json` | Choose output format.                                                                 |

ALWAYS remember the `--` separator. `cargo test foo` filters by name; `cargo test -- foo` is also valid (Cargo forwards positional args to the harness), but `cargo test --test-threads=1` is a Cargo error because the flag is unknown to Cargo; you need `cargo test -- --test-threads=1`.

---

## 3. `cargo nextest` flag reference

| Command / flag                       | Meaning                                                              |
|--------------------------------------|----------------------------------------------------------------------|
| `cargo nextest run`                  | Build and run all non-doc tests in the current package.              |
| `cargo nextest run --workspace`      | Run all non-doc tests in every workspace member.                     |
| `cargo nextest run -E '<filterset>'` | Run only tests matching the filterset DSL expression.                |
| `cargo nextest run --profile ci`     | Use the `[profile.ci]` section from `.config/nextest.toml`.          |
| `cargo nextest run --retries <N>`    | Retry failing tests up to N times.                                   |
| `cargo nextest list`                 | List tests without running them.                                     |
| `cargo nextest archive --archive-file <PATH>` | Build and bundle tests for later replay.                    |

Common filterset expressions:

| Filterset                          | Matches                                                          |
|------------------------------------|-------------------------------------------------------------------|
| `test(parse)`                      | Tests whose name contains `parse`.                                |
| `test(=adds_two_positives)`        | Test whose name is exactly `adds_two_positives`.                  |
| `binary(integration_test)`         | Tests inside the `integration_test` binary.                       |
| `package(my_crate)`                | Tests inside the `my_crate` package.                              |
| `not test(slow)`                   | Everything except tests with `slow` in the name.                  |

Installation (Linux/macOS, one-shot binary):

```bash
curl -LsSf https://get.nexte.st/latest/linux | tar zxf - -C ${CARGO_HOME:-~/.cargo}/bin
# macOS:
curl -LsSf https://get.nexte.st/latest/mac   | tar zxf - -C ${CARGO_HOME:-~/.cargo}/bin
# Via cargo-binstall:
cargo binstall cargo-nextest --secure
```

Doc tests are NOT supported by nextest. ALWAYS keep a `cargo test --doc` step alongside `cargo nextest run` in CI.

---

## 4. Doc-test info-string reference

A doc-test code block is fenced with three backticks plus an optional comma-separated list of info-strings.

| Info-string         | Behaviour                                                                              |
|---------------------|----------------------------------------------------------------------------------------|
| (empty) / `rust`    | Default. Compile and run.                                                              |
| `ignore`            | Skip compilation and execution. Last resort; prefer the typed alternatives below.      |
| `no_run`            | Compile to type-check, but do not execute (good for networked or `loop {}` code).      |
| `should_panic`      | The block MUST panic at runtime to pass.                                               |
| `compile_fail`      | The block MUST fail to compile to pass.                                                |
| `edition2015` etc.  | Compile this block under the named edition (`edition2018`, `edition2021`, `edition2024`). |
| `standalone_crate`  | Do not merge with sibling doctests; useful when relying on panic line numbers.          |
| `ignore-<target>`   | Skip on the named target triple (`ignore-windows`, `ignore-x86_64`, ...).               |

Mechanics rustdoc applies before compiling each block (paraphrased from the rustdoc reference):

1. Inserts `#[allow(unused_variables, unused_assignments, unused_mut, unused_attributes, dead_code)]`.
2. Adds any `#![doc(test(attr(...)))]` crate-level attributes.
3. Injects `extern crate <yourcrate>;` unless `#![doc(test(no_crate_inject))]` is set.
4. Wraps the snippet in `fn main() { ... }` if no `fn main` is present.

Therefore: a doc-test that needs `?` either declares its own `fn main() -> Result<...>` (visible in rendered docs) or hides one with `#`-prefixed lines:

```rust
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let n: i32 = "42".parse()?;
/// assert_eq!(n, 42);
/// # Ok(()) }
/// ```
```

ALWAYS gate doc-tests that contain placeholders (`...`, partial fns) with `ignore` or `compile_fail`. NEVER let an example silently rot: if `cargo test --doc` no longer compiles a snippet, the comment now lies about the API.

---

## 5. Recommended `[dev-dependencies]` for a fully-equipped test suite

```toml
[dev-dependencies]
tokio   = { version = "1", features = ["macros", "rt-multi-thread"] }
proptest = "1"
insta    = "1"
criterion = { version = "0.5", features = ["html_reports"] }
tempfile = "3"

[[bench]]
name = "my_bench"
harness = false
```

Notes:

- `tokio` features: `macros` enables `#[tokio::test]`; `rt-multi-thread` is needed if you specify `flavor = "multi_thread"`.
- `harness = false` is REQUIRED for criterion benches; without it Cargo links libtest and the criterion macros conflict.
- `tempfile::TempDir` creates an isolated, auto-deleted directory; ALWAYS use it instead of hard-coded `/tmp/foo` paths so tests are parallel-safe.

---

## 6. Test-relevant Cargo profile knobs

See [[rust-impl-cargo-project]] for the full profile reference. Test-specific notes:

| Profile          | Inherits | When it is used                                          |
|------------------|----------|----------------------------------------------------------|
| `[profile.test]` | `dev`    | `cargo test`, `cargo nextest run`.                       |
| `[profile.bench]`| `release`| `cargo bench` and criterion benches.                     |

Common overrides:

```toml
[profile.test]
opt-level = 1            # speeds up heavy property tests vs default 0
debug = true             # keep debug info for nicer backtraces

[profile.bench]
debug = true             # required for flamegraph profiling of benches
```

---

## 7. Sources (verified 2026-05-19)

- The Rust Book ch. 11.1 "Writing Tests": https://doc.rust-lang.org/book/ch11-01-writing-tests.html
- The Rust Book ch. 11.2 "Controlling How Tests Are Run": https://doc.rust-lang.org/book/ch11-02-running-tests.html
- The Rust Book ch. 11.3 "Test Organization": https://doc.rust-lang.org/book/ch11-03-test-organization.html
- The Cargo Book, `cargo test`: https://doc.rust-lang.org/cargo/commands/cargo-test.html
- rustdoc, "Documentation tests": https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html
- cargo-nextest home: https://nexte.st/
- cargo-nextest install: https://nexte.st/docs/installation/pre-built-binaries/
- tokio `#[tokio::test]`: https://docs.rs/tokio/latest/tokio/attr.test.html
- criterion user guide: https://bheisler.github.io/criterion.rs/book/
- proptest book: https://proptest-rs.github.io/proptest/
- insta docs: https://docs.rs/insta/latest/insta/
