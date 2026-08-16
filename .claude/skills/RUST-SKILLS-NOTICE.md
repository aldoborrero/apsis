# Vendored Rust skills — attribution

The `rust-*` skills in this directory are a **curated subset** of
**[Impertio-Studio/Rust-Claude-Skill-Package](https://github.com/Impertio-Studio/Rust-Claude-Skill-Package)**
(**MIT**, author OpenAEC-Foundation), targeting **Rust 1.85+ / edition 2024** —
matching apsis's toolchain.

Curated for a library/systems crate: `rust-core-toolchain`, `rust-core-type-system`,
`rust-syntax-edition-2024`, `rust-syntax-traits`, `rust-syntax-iterators-closures`,
`rust-syntax-lifetimes`, `rust-errors-thiserror-anyhow`, `rust-errors-borrow-checker`,
`rust-impl-error-handling`, `rust-impl-testing`, `rust-impl-serde`, `rust-impl-workspaces`.

Omitted (add when the coordinator/worker in specs 002/003 need them): the async / tokio /
concurrency / CLI / no-std / FFI / procedural-macro / unsafe skills.

Each `SKILL.md` keeps its `license: MIT` frontmatter. Some `[[wiki-links]]` point to
sibling skills that were not vendored — harmless (they simply have no local target).
