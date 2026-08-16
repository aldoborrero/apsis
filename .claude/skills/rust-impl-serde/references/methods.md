# Methods Reference: serde attributes and APIs

Complete catalog of attributes and APIs for serde 1.x, verified against [serde.rs](https://serde.rs/) on 2026-05-19.

---

## Table of contents

1. Container attributes (full list)
2. Field attributes (full list)
3. Variant attributes (full list)
4. The `Serialize` and `Deserialize` traits
5. The Visitor pattern for manual `Deserialize`
6. `serde_json` core functions
7. Common third-party `with` modules

---

## 1. Container attributes

Applied to the type definition: `#[derive(Serialize, Deserialize)] #[serde(...)] struct Foo { ... }`.

| Attribute | Exact form | Where allowed |
|-----------|-----------|---------------|
| `rename = "name"` | string | struct, enum |
| `rename(serialize = "s", deserialize = "d")` | named args | struct, enum |
| `rename_all = "case"` | one of `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, `SCREAMING-KEBAB-CASE` | struct, enum (renames variants) |
| `rename_all(serialize = "case", deserialize = "case")` | split control | struct, enum |
| `rename_all_fields = "case"` | same eight values | enum only; renames fields *inside* each struct variant |
| `deny_unknown_fields` | flag | struct, enum |
| `tag = "field"` | string | enum (internally tagged) |
| `tag = "t", content = "c"` | two strings | enum (adjacently tagged) |
| `untagged` | flag | enum |
| `bound = "T: MyTrait"` | where-clause | struct, enum |
| `bound(serialize = "...", deserialize = "...")` | split bounds | struct, enum |
| `default` | flag | struct (NOT enum) |
| `default = "fn"` | function path, signature `fn() -> Self` | struct |
| `remote = "::path::Type"` | path | struct, enum |
| `transparent` | flag, single-field newtype only | struct |
| `from = "Type"` | type, must `impl From<Type> for Self` | struct, enum (deserialize via intermediate) |
| `try_from = "Type"` | type, `TryFrom<Type>` | struct, enum |
| `into = "Type"` | type, `Into<Type>` impl on Self | struct, enum (serialize via intermediate, clones) |
| `crate = "::path::to::serde"` | path | struct, enum (re-exporting crates) |
| `expecting = "string"` | string | enum, error message |
| `variant_identifier` | flag | enum (for `Identifier` deserialization) |
| `field_identifier` | flag | enum (for field-name deserialization) |

NEVER use `default` on an enum: serde rejects it at compile time. Use `try_from` to route through an intermediate type that has a default.

---

## 2. Field attributes

Applied to a single field: `struct Foo { #[serde(...)] field: T }`.

| Attribute | Exact form | Notes |
|-----------|-----------|-------|
| `rename = "name"` | string | One name on the wire, both directions. |
| `rename(serialize = "s", deserialize = "d")` | split |
| `alias = "name"` | string, repeatable | Deserialize-only accept-list. Stacks. |
| `default` | flag | Calls `T::default()` when absent. |
| `default = "fn"` | path, `fn() -> T` | Custom default function. |
| `flatten` | flag | Inline contents into parent. Cannot combine with `deny_unknown_fields` on the parent. |
| `skip` | flag | Skip both directions; needs `Default`. |
| `skip_serializing` | flag |
| `skip_deserializing` | flag | Needs `Default` or `default = "fn"`. |
| `skip_serializing_if = "fn"` | predicate `fn(&T) -> bool` | Skip only when true. Common: `"Option::is_none"`, `"Vec::is_empty"`, `"std::ops::Not::not"` (for `bool`). |
| `serialize_with = "fn"` | path, `fn<S: Serializer>(&T, S) -> Result<S::Ok, S::Error>` |
| `deserialize_with = "fn"` | path, `fn<'de, D: Deserializer<'de>>(D) -> Result<T, D::Error>` |
| `with = "module"` | path to a module exposing `serialize` and `deserialize` functions |
| `borrow` | flag | Zero-copy from input buffer. |
| `borrow = "'a + 'b"` | explicit lifetimes | When the field has multiple lifetime parameters. |
| `bound = "T: Trait"` | where-clause | Per-field generic bound override. |
| `getter = "fn"` | path | For `#[serde(remote = ...)]` derive: how to read a private field. |

---

## 3. Variant attributes

Applied to an enum variant: `enum E { #[serde(...)] Variant }`.

| Attribute | Form | Notes |
|-----------|------|-------|
| `rename = "name"` | string | Rename one variant. |
| `rename(serialize = "...", deserialize = "...")` | split |
| `alias = "name"` | string, repeatable | Deserialize-only accept-list per variant. |
| `rename_all = "case"` | same eight values | Apply to fields *inside* this struct variant only. |
| `skip` | flag | Variant not serialized nor accepted. |
| `skip_serializing` | flag |
| `skip_deserializing` | flag |
| `serialize_with = "fn"` | path |
| `deserialize_with = "fn"` | path |
| `with = "module"` | path |
| `bound = "..."` | where-clause |
| `borrow` | flag (newtype variant containing a borrowed value) |
| `other` | flag (one variant per enum, catches anything unknown; only on internally tagged enums) |
| `untagged` | flag, per-variant in `field_identifier` enums |

---

## 4. The traits

```rust
pub trait Serialize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer;
}

pub trait Deserialize<'de>: Sized {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>;
}

pub trait DeserializeOwned: for<'de> Deserialize<'de> {}
```

`DeserializeOwned` is the trait alias to require when the value must not borrow from the input buffer. Use it on background-task / channel boundaries where the input lifetime does not survive.

The `Serializer` and `Deserializer` traits are *format-implementor* APIs. Most user code never names them: the derive macros emit the calls, and `serde_json::to_string` / `from_str` pick the right serializer / deserializer behind the scenes.

---

## 5. The Visitor pattern for manual `Deserialize`

```rust
use serde::de::{self, Deserialize, Deserializer, Visitor, MapAccess};
use std::fmt;

struct Point { x: i32, y: i32 }

impl<'de> Deserialize<'de> for Point {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PointVisitor;

        impl<'de> Visitor<'de> for PointVisitor {
            type Value = Point;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("struct Point with x and y")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Point, A::Error> {
                let mut x = None;
                let mut y = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "x" => x = Some(map.next_value()?),
                        "y" => y = Some(map.next_value()?),
                        other => return Err(de::Error::unknown_field(other, &["x", "y"])),
                    }
                }
                Ok(Point {
                    x: x.ok_or_else(|| de::Error::missing_field("x"))?,
                    y: y.ok_or_else(|| de::Error::missing_field("y"))?,
                })
            }
        }

        deserializer.deserialize_struct("Point", &["x", "y"], PointVisitor)
    }
}
```

`Visitor` has one method per *serde data-model* type: `visit_bool`, `visit_i32`, `visit_str`, `visit_borrowed_str`, `visit_seq`, `visit_map`, `visit_unit`, `visit_some`, `visit_none`, `visit_enum`. Implement only the ones your type can be built from. The default impls return "invalid type" errors, which serde_json renders as the familiar `invalid type: integer 1, expected struct Point`.

---

## 6. `serde_json` core functions

| Function | Signature gist | Use |
|----------|----------------|-----|
| `to_string` | `(value) -> Result<String>` | Compact JSON. |
| `to_string_pretty` | `(value) -> Result<String>` | Indented JSON (2 spaces). |
| `to_vec` | `(value) -> Result<Vec<u8>>` | Same as `to_string` but UTF-8 bytes; no extra String. |
| `to_writer` | `(W, value) -> Result<()>` | Stream into any `io::Write`. |
| `to_value` | `(value) -> Result<Value>` | Move into the dynamic `Value` representation. |
| `from_str` | `(&str) -> Result<T>` | Parse JSON text. |
| `from_slice` | `(&[u8]) -> Result<T>` | Parse JSON bytes. |
| `from_reader` | `(R) -> Result<T>` | Parse from any `io::Read`. *Buffers internally*; for a long-lived stream prefer `Deserializer::from_reader(R).into_iter::<T>()`. |
| `from_value` | `(Value) -> Result<T>` | Move out of the dynamic representation. |
| `json!` (macro) | `json!({...})` | Inline literal `Value` construction with interpolation. |

NEVER call `from_reader` on an unbounded socket: it buffers the entire input. For streamed JSON: build a `Deserializer::from_reader(reader).into_iter::<T>()` and consume records one by one.

---

## 7. Common third-party `with` modules

Battle-tested helpers that ship as part of larger crates:

- `chrono::serde::ts_seconds`: Unix timestamp (signed) as i64.
- `chrono::serde::ts_milliseconds`: as i64 milliseconds.
- `chrono::serde::ts_seconds_option`: same for `Option<DateTime<Utc>>`.
- `uuid::serde::compact`: emit / parse UUIDs as 16-byte arrays (for binary formats).
- `uuid::serde::simple`: emit as 32 hex chars, no dashes.
- `uuid::serde::urn`: emit as `urn:uuid:...`.
- `serde_with::hex`: `Vec<u8>` <-> hex string. Requires the `serde_with` crate.
- `serde_with::base64`: `Vec<u8>` <-> base64. Requires `serde_with`.
- `serde_with::DisplayFromStr`: serialize via `Display`, deserialize via `FromStr` (with the `#[serde_as]` macro).

The `serde_with` crate (v3.x) is the canonical source of additional adapters. Use the `#[serde_as(as = "DisplayFromStr")]` family of attributes when the standard `with` / `serialize_with` machinery becomes verbose.

---

## Reference links

- Container attributes: <https://serde.rs/container-attrs.html>
- Field attributes: <https://serde.rs/field-attrs.html>
- Variant attributes: <https://serde.rs/variant-attrs.html>
- Custom serialization: <https://serde.rs/custom-serialization.html>
- Custom deserialization: <https://serde.rs/impl-deserialize.html>
- `serde_json` API: <https://docs.rs/serde_json/latest/serde_json/>
- `serde_with` API: <https://docs.rs/serde_with/latest/serde_with/>
