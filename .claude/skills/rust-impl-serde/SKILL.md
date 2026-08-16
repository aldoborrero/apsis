---
name: rust-impl-serde
description: >
  Use when the user derives Serialize / Deserialize, writes custom impls, picks enum representation (externally / internally / adjacently / untagged), uses `flatten` / `with` / `rename_all`, or chooses between JSON / TOML / YAML / bincode.
  Prevents enum-rep ambiguity, forgetting `serde(borrow)` for zero-copy `&'a str`, picking untagged with overlapping fields, and deriving without enabling the `derive` feature.
  Covers: Serialize / Deserialize derives, container attributes (rename_all, deny_unknown_fields, transparent, default, tag, content), field attributes (rename, skip, skip_serializing_if, default, with, serialize_with, deserialize_with, flatten, alias, borrow), enum representations, custom impl when derive insufficient, serde_json common patterns, format-crate selection (TOML / YAML / bincode / MessagePack).
  Keywords: serde, Serialize, Deserialize, "#[derive(Serialize, Deserialize)]", rename, rename_all, "#[serde(...)]", flatten, with, "serialize_with", "deserialize_with", skip, "skip_serializing_if", default, alias, "serde(borrow)", tag, content, untagged, "internally tagged", "externally tagged", "adjacently tagged", serde_json, toml, serde_yaml, bincode, "deny_unknown_fields", "transparent", "custom serde impl".
license: MIT
compatibility: "Designed for Claude Code. Requires Rust 1.85+, edition 2024."
metadata:
  author: OpenAEC-Foundation
  version: "1.0"
---

# rust-impl-serde

The **structured data interchange** skill: how to derive `Serialize` / `Deserialize`, shape the wire format with container and field attributes, pick the right enum representation, write a custom impl when the derive is not enough, and choose between JSON, TOML, YAML, bincode, and MessagePack. Serde is a compile-time framework: no runtime reflection, zero overhead, the data-structure / data-format interaction "can be completely optimized away by the Rust compiler" (per [serde.rs](https://serde.rs/)).

Cross-references: [[rust-impl-cargo-project]] (feature flags, the `derive` feature), [[rust-syntax-traits]] (`Serialize` / `Deserialize` as trait bounds, generic impls), [[rust-syntax-lifetimes]] (zero-copy deserialization with `#[serde(borrow)]` and `'de`).

---

## When to use this skill

- User adds `#[derive(Serialize, Deserialize)]` and gets `error: cannot find derive macro` (forgot the `derive` feature).
- User asks how to rename fields, accept legacy names, or convert all field names to `camelCase` at once.
- User has an enum and asks which JSON shape to emit: tagged, untagged, adjacently tagged.
- User wants to flatten a sub-struct into the parent shape, or pull arbitrary extra fields into a `HashMap`.
- User wants zero-copy deserialization with `&'a str` and gets `error: implementation of Deserialize is not general enough`.
- User asks whether to choose `serde_json`, `toml`, `serde_yaml`, `bincode`, or `rmp-serde` for a given task.
- User asks how to write a manual `Serialize` / `Deserialize` impl when the derive cannot express the format.
- User mixes `#[serde(rename = ...)]` with `#[serde(alias = ...)]` for a schema migration.

---

## Cargo.toml setup: enable the `derive` feature

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"   # or toml, serde_yaml, bincode, rmp-serde
```

ALWAYS enable `features = ["derive"]` on the `serde` dependency. Without it, `#[derive(Serialize, Deserialize)]` does not resolve because the proc-macro lives in a separate crate (`serde_derive`) that is gated behind that feature.

NEVER add `serde_derive` as a direct dependency. Always go through `serde`'s `derive` feature so the versions stay in lockstep.

For `no_std` + alloc, use `serde = { version = "1", default-features = false, features = ["derive", "alloc"] }`. Internally / adjacently tagged and untagged enums require `alloc` for deserialization (see enum-representations section).

---

## The two traits in one minute

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
struct Point {
    x: i32,
    y: i32,
}

let p = Point { x: 1, y: 2 };

let json = serde_json::to_string(&p)?;           // "{\"x\":1,\"y\":2}"
let back: Point = serde_json::from_str(&json)?;
```

- `Serialize`: turn a Rust value into a sequence of calls on a `Serializer` (the format-specific encoder).
- `Deserialize<'de>`: build a Rust value from a sequence of calls into a `Deserializer` (the format-specific decoder). The `'de` lifetime is the lifetime of the input buffer being borrowed.

ALWAYS derive both traits unless you only need one direction; the cost is one extra impl block at compile time, zero at runtime.

---

## Container attributes: shape the whole struct or enum

Container attributes apply to the top-level `struct` or `enum`. Listed with the **exact** values accepted by serde (from `serde.rs/container-attrs.html`):

| Attribute | Accepted values / form | Effect |
|-----------|-----------------------|--------|
| `rename = "..."` | string literal | Change the type or variant name on the wire. |
| `rename_all = "..."` | `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, `SCREAMING-KEBAB-CASE` | Rewrite all field / variant names in the chosen case. |
| `rename_all_fields = "..."` | same eight values | Same as `rename_all`, but applied to the *fields of each struct variant* in an enum. |
| `deny_unknown_fields` | flag (no value) | Error on extra fields during deserialization. |
| `tag = "..."` | string literal | Switch the enum to internally-tagged representation. |
| `tag = "..." , content = "..."` | two string literals | Adjacently-tagged representation. |
| `untagged` | flag | Untagged enum representation. |
| `default` / `default = "fn"` | flag or function name | Missing-field defaults from `Default` or a custom `fn() -> T`. |
| `transparent` | flag (newtype only) | Serialize / deserialize the single inner field directly, with no wrapper. |
| `from = "Type"` / `into = "Type"` / `try_from = "Type"` | type name | Route serialization through an intermediate type. |
| `bound = "..."` | where-clause string | Override the generated `T: Serialize` / `T: Deserialize` bounds for generic structs. |
| `remote = "..."` | path of a remote type | Derive impls for a type defined in another crate (orphan-rule workaround). |
| `expecting = "..."` | string literal | Customise the "expected ..." part of error messages. |
| `crate = "..."` | path | Override the path used to reference the `serde` crate (re-exports). |

### `rename_all` is the most common one

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiRequest {
    user_id: u64,        // <- "userId" on the wire
    access_token: String,// <- "accessToken"
}
```

ALWAYS use `rename_all` instead of `#[serde(rename = "...")]` on every individual field when the only goal is a case convention. NEVER mix the two for one struct: pick `rename_all` for the whole struct, and use per-field `rename` only for *exceptions* (legacy spelling, reserved word, etc.).

### `deny_unknown_fields` is the strict mode

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    port: u16,
    host: String,
}
```

ALWAYS apply `deny_unknown_fields` to *config* structs where unknown keys signal a user typo. NEVER apply it to API-response structs that may grow new fields over time; the strict mode will break clients the day the server adds a key.

### `transparent` for newtypes

```rust
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct UserId(u64);
```

Without `transparent`, a `UserId(7)` serializes to JSON as `7` *only* because newtype-struct support is built in. The `transparent` flag is required for *named* single-field structs (`struct UserId { id: u64 }`) to omit the wrapper object.

---

## Field attributes: shape individual fields

Field attributes apply to a single field inside a struct or struct variant (from `serde.rs/field-attrs.html`):

| Attribute | Form | Effect |
|-----------|------|--------|
| `rename = "..."` | string | Rename one field on the wire. |
| `alias = "..."` | string, repeatable | Accept additional names *only on deserialize*. The Rust name and any `rename` value remain valid; aliases stack. |
| `default` / `default = "fn"` | flag or function | Use `Default::default()` or `fn() -> T` if absent on deserialize. |
| `skip` | flag | Skip the field in both directions. Requires `Default` for deserialization. |
| `skip_serializing` | flag | Skip on serialize only. |
| `skip_deserializing` | flag | Skip on deserialize only. Requires `Default` (or `default = "fn"`). |
| `skip_serializing_if = "fn"` | predicate `fn(&T) -> bool` | Skip only when predicate returns true. |
| `serialize_with = "fn"` | path | Override `Serialize::serialize` for this field only. |
| `deserialize_with = "fn"` | path | Override `Deserialize::deserialize` for this field only. |
| `with = "module"` | path to module | Sugar for `serialize_with = "module::serialize"` + `deserialize_with = "module::deserialize"`. |
| `flatten` | flag | Inline this field's contents into the parent map. |
| `borrow` / `borrow = "'a + 'b"` | flag or lifetime list | Mark the field as borrowing from the input (zero-copy `&'a str`, `Cow<'a, str>`, `&'a [u8]`). |
| `bound = "..."` | where-clause | Per-field generic bound override. |
| `getter = "fn"` | path | For `remote = ...` types with private fields: how to read the field. |

### `alias` vs `rename`: the schema-migration rule

```rust
#[derive(Deserialize)]
struct User {
    #[serde(rename = "userId", alias = "user_id", alias = "id")]
    user_id: u64,
}
```

- `rename` controls *the* wire name in both directions.
- `alias` controls *additional* accepted names on deserialize only.

ALWAYS pair a `rename` (forward-compatible new name) with one or more `alias` entries for old names during migration. NEVER change `rename` from `"user_id"` to `"userId"` without adding `#[serde(alias = "user_id")]`; existing payloads will reject.

### `skip_serializing_if` for `Option` fields

```rust
#[derive(Serialize)]
struct Patch {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
}
```

ALWAYS use `skip_serializing_if = "Option::is_none"` for `Option` fields in JSON-patch / partial-update payloads. NEVER serialize a literal `"name": null` when the caller meant "do not change name".

### `flatten`: pull a sub-struct up

```rust
#[derive(Serialize, Deserialize)]
struct Page<T> {
    items: Vec<T>,
    #[serde(flatten)]
    pagination: Pagination,
}

#[derive(Serialize, Deserialize)]
struct Pagination {
    page: u32,
    total: u32,
}
```

Wire shape: `{"items":[...],"page":1,"total":42}` (the `pagination` key is gone).

NEVER combine `#[serde(flatten)]` with `#[serde(deny_unknown_fields)]` on the *same* parent: flatten is internally implemented by capturing "extra" fields, which `deny_unknown_fields` rejects. The compiler will not warn; the runtime error is `unknown field <flattened-field>, expected one of ...`.

### `borrow` for zero-copy

```rust
#[derive(Deserialize)]
struct Slice<'a> {
    #[serde(borrow)]
    name: &'a str,
}
```

ALWAYS annotate `&'a str`, `&'a [u8]`, and `Cow<'a, str>` fields with `#[serde(borrow)]` when the goal is zero-copy. Without it, serde tries to satisfy `Deserialize<'static>` and the compile error is `implementation of Deserialize is not general enough`. The lifetime `'a` must outlive the deserializer's `'de`.

---

## Enum representations: pick exactly one

Four representations, in increasing JSON noisiness (verified against `serde.rs/enum-representations.html`):

| Representation | Attribute | JSON example for `Message::Move { x: 1, y: 2 }` | Limitations |
|----------------|-----------|-------------------------------------------------|-------------|
| Externally tagged (default) | none | `{"Move": {"x": 1, "y": 2}}` | None; works everywhere, including binary and `no_std + no_alloc`. |
| Internally tagged | `#[serde(tag = "type")]` | `{"type": "Move", "x": 1, "y": 2}` | NO tuple variants. Newtype variants must wrap a struct or map. Requires `alloc` for deserialize. |
| Adjacently tagged | `#[serde(tag = "t", content = "c")]` | `{"t": "Move", "c": {"x": 1, "y": 2}}` | Any variant type. Requires `alloc` for deserialize. |
| Untagged | `#[serde(untagged)]` | `{"x": 1, "y": 2}` | Variants must be *structurally* distinguishable. First successful match wins. Requires `alloc`. |

### Decision tree

```
Does the wire format already exist?
  YES -> match it exactly (no choice).
  NO  -> continue.

Is this a binary or no_alloc target?
  YES -> externally tagged (default). DONE.
  NO  -> continue.

Do you want the variant tag inline with the data (e.g. `{"type": "..."}`)?
  YES, and all variants are struct-shaped -> internally tagged.
  YES, but some variants are tuples or primitives -> adjacently tagged.
  NO, and the variants are structurally distinct -> untagged.
  NO, and you want the cleanest fallback -> externally tagged.
```

NEVER pick `untagged` when two variants could match the same input. Serde tries variants in source order and returns the first one that deserializes successfully, which is a silent footgun.

---

## When the derive is not enough: manual impl

The derive covers ~95% of cases. Write a manual `Serialize` / `Deserialize` impl when:

1. You need a format that does not fit any combination of attributes (e.g. a date-time stored as both an integer and a string depending on a header).
2. You are implementing serde for a type that contains a non-`Serialize` field that you compute on the fly.
3. You need to read from the deserializer in a stateful way (peek, lookahead).

```rust
use serde::ser::{Serialize, Serializer, SerializeStruct};

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("Color", 3)?;
        s.serialize_field("r", &self.r)?;
        s.serialize_field("g", &self.g)?;
        s.serialize_field("b", &self.b)?;
        s.end()
    }
}
```

ALWAYS prefer `with = "module"` + `serialize_with` / `deserialize_with` when only *one field* needs a custom shape. Only write a full impl when the whole type needs custom logic.

See `references/methods.md` for the full Visitor pattern used in manual `Deserialize`.

---

## Format crate matrix: choose the wire encoding

| Crate | Version (May 2026) | Wire format | When to choose |
|-------|--------------------|-------------|----------------|
| `serde_json` | 1.x | JSON (UTF-8 text) | Web APIs, config files when humans edit, log lines. The default. |
| `toml` | 0.8.x | TOML | Cargo.toml-style config, anything where comments matter. |
| `serde_yaml` | 0.9.x | YAML | CI/CD configs, k8s manifests. Note: the crate is in maintenance mode; consider `serde_yml` as the active fork. |
| `bincode` | 2.x | Compact binary | Internal IPC, caches, on-disk format owned by the same Rust program on both sides. NOT portable across languages. |
| `rmp-serde` | 1.x | MessagePack | Cross-language compact binary, smaller and faster than JSON. |
| `ciborium` | 0.2.x | CBOR (RFC 8949) | When the spec mandates CBOR (e.g. IoT, COSE, WebAuthn). |
| `serde_qs` | 0.13.x | URL query strings | Parsing `?a=1&b=2&nested[x]=1`-style query params. |
| `csv` | 1.x | CSV | Tabular data; uses serde for row mapping. |

ALWAYS pick the simplest format that fits the actual constraint. NEVER reach for `bincode` to "make JSON faster" unless you have measured a serialization bottleneck; JSON is fast enough for almost all web APIs.

---

## Quick recipes

### Round-trip a struct through JSON

```rust
let v = Point { x: 1, y: 2 };
let s = serde_json::to_string(&v)?;          // owned String
let s_pretty = serde_json::to_string_pretty(&v)?;
let back: Point = serde_json::from_str(&s)?;
```

### Untyped JSON: `serde_json::Value`

```rust
let v: serde_json::Value = serde_json::from_str(r#"{"a":1}"#)?;
let a = v["a"].as_i64().ok_or("not an int")?;
```

Use `Value` for *truly* dynamic JSON (unknown shape until runtime). For known shape: always a typed struct.

### Move a typed value through `Value` for inspection

```rust
let value: serde_json::Value = serde_json::to_value(&point)?;
let back: Point = serde_json::from_value(value)?;
```

### Read a TOML file

```rust
let s = std::fs::read_to_string("Config.toml")?;
let cfg: Config = toml::from_str(&s)?;
```

### Custom `with = ` module for a chrono `DateTime`

```rust
mod ts_seconds {
    use chrono::{DateTime, Utc, TimeZone};
    use serde::{Deserializer, Serializer, Deserialize};

    pub fn serialize<S: Serializer>(dt: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_i64(dt.timestamp())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let secs = i64::deserialize(d)?;
        Ok(Utc.timestamp_opt(secs, 0).single().expect("valid unix ts"))
    }
}

#[derive(Serialize, Deserialize)]
struct Event {
    #[serde(with = "ts_seconds")]
    occurred_at: chrono::DateTime<chrono::Utc>,
}
```

(`chrono::serde::ts_seconds` ships this for free; the snippet shows the pattern.)

### Catch-all extra fields with `flatten` + `HashMap`

```rust
#[derive(Serialize, Deserialize)]
struct Loose {
    id: u64,
    #[serde(flatten)]
    extra: std::collections::HashMap<String, serde_json::Value>,
}
```

Any unknown JSON field falls into `extra`. Symmetric on serialize: the map's entries become top-level keys.

---

## Validation checklist

Before merging any code that uses serde:

1. `serde = { version = "1", features = ["derive"] }` (NOT `serde_derive` directly).
2. Every `&'a str` / `&'a [u8]` / `Cow<'a, _>` field on a `Deserialize` struct has `#[serde(borrow)]`.
3. No enum has both `#[serde(untagged)]` and structurally-overlapping variants.
4. No field has both `#[serde(flatten)]` and a parent `#[serde(deny_unknown_fields)]`.
5. Every `Option` in a JSON-patch / partial-update payload has `#[serde(skip_serializing_if = "Option::is_none")]`.
6. Schema migrations use `#[serde(alias = "old_name")]`, not a silent `#[serde(rename = ...)]` swap.
7. Config-file structs use `#[serde(deny_unknown_fields)]`; API-response structs do not.
8. `serde_json::Value` appears only at trust boundaries; the rest of the code uses typed structs.
9. `bincode` is only used for Rust-to-Rust traffic; cross-language traffic uses JSON, MessagePack, or CBOR.

---

## Reference links

- Serde overview: <https://serde.rs/>
- Container attributes: <https://serde.rs/container-attrs.html>
- Field attributes: <https://serde.rs/field-attrs.html>
- Variant attributes: <https://serde.rs/variant-attrs.html>
- Enum representations: <https://serde.rs/enum-representations.html>
- Data model: <https://serde.rs/data-model.html>
- `serde_json` docs: <https://docs.rs/serde_json/latest/serde_json/>
- `toml` docs: <https://docs.rs/toml/latest/toml/>
- `bincode` docs: <https://docs.rs/bincode/latest/bincode/>

For deeper material: see `references/methods.md` (every container / field / variant attribute with exact syntax), `references/examples.md` (round-trip recipes), `references/anti-patterns.md` (footguns and fixes).
