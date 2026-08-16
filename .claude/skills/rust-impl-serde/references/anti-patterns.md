# Anti-patterns: serde mistakes and their fixes

Every entry is a concrete failure mode observed in real code, the diagnostic, and the correct alternative.

---

## AP-1: Deriving without enabling the `derive` feature

### Bad

```toml
# Cargo.toml
[dependencies]
serde = "1"
```

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]   // error: cannot find derive macro `Serialize`
struct Point { x: i32, y: i32 }
```

### Why it fails

The `Serialize` and `Deserialize` derive macros live in a separate proc-macro crate (`serde_derive`) re-exported through `serde::__private`. The re-export is gated behind the `derive` feature. Without the feature, `serde::Serialize` is the *trait*, not the macro.

### Fix

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
```

ALWAYS enable `features = ["derive"]`. NEVER add `serde_derive` as a separate dependency: the versions can drift and the macro-generated code will fail to compile against a mismatched `serde` trait definition.

---

## AP-2: `#[serde(untagged)]` with overlapping variants

### Bad

```rust
#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum Id {
    Numeric(u64),
    AnyValue(serde_json::Value),
}

let parsed: Id = serde_json::from_str("42").unwrap();
// Always Numeric, never AnyValue, no matter what the caller intended.
```

### Why it fails

Serde tries variants top-down and returns the first successful deserialization. `u64` matches `42`, so the `AnyValue` variant is unreachable for any numeric input. There is no compile-time check and no warning at runtime.

### Fix

Make the variants structurally exclusive. Add a tag (internally or adjacently tagged) so each variant has a unique discriminator:

```rust
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", content = "value")]
enum Id {
    Numeric(u64),
    AnyValue(serde_json::Value),
}
```

Or, if you must keep untagged, ensure the structure differs: e.g. `Id::Tagged { kind: String, value: u64 }` vs `Id::Free(serde_json::Value)`. NEVER stack two untagged variants where one is a strict subtype of the other.

---

## AP-3: `#[serde(flatten)]` with `#[serde(deny_unknown_fields)]`

### Bad

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page<T> {
    items: Vec<T>,
    #[serde(flatten)]
    extra: std::collections::HashMap<String, serde_json::Value>,
}

let r: Page<u32> = serde_json::from_str(r#"{"items":[1],"page":2}"#)?;
// Runtime error: unknown field `page`, expected `items`
```

### Why it fails

`flatten` is internally implemented by deserializing the parent's *known* fields, then collecting the leftovers into the flattened field. `deny_unknown_fields` rejects those leftovers before they reach the flatten capture.

### Fix

Drop one of the two. If extra fields must be captured, remove `deny_unknown_fields`:

```rust
#[derive(Deserialize)]
struct Page<T> {
    items: Vec<T>,
    #[serde(flatten)]
    extra: std::collections::HashMap<String, serde_json::Value>,
}
```

If strict mode is required, drop `flatten` and enumerate every accepted field explicitly. NEVER combine the two attributes on the same struct.

---

## AP-4: Forgetting `#[serde(borrow)]` on `&'a str`

### Bad

```rust
#[derive(Deserialize)]
struct Header<'a> {
    name: &'a str,    // missing #[serde(borrow)]
    value: &'a str,
}
// error: implementation of `Deserialize` is not general enough
//        `Header<'_>` must implement `Deserialize<'0>`, for any lifetime `'0`...
```

### Why it fails

`Deserialize<'de>` carries a generic lifetime `'de` for the input buffer. A `&'a str` field needs to *borrow* from `'de`, which serde does not infer automatically. Without `borrow`, the derive emits `impl<'de, 'a> Deserialize<'de> for Header<'a>` *without* a `'a: 'de` constraint, so the lifetimes are unrelated and the impl is not general enough.

### Fix

```rust
#[derive(Deserialize)]
struct Header<'a> {
    #[serde(borrow)]
    name: &'a str,
    #[serde(borrow)]
    value: &'a str,
}
```

The `borrow` attribute ties the field's lifetime to `'de`. ALWAYS annotate every `&'a str`, `&'a [u8]`, and `Cow<'a, _>` field in a `Deserialize` struct.

---

## AP-5: Using `rename` instead of `alias` for schema migration

### Bad

```rust
// v1
#[derive(Deserialize)]
struct User { user_id: u64 }

// v2 (silently rename)
#[derive(Deserialize)]
struct User {
    #[serde(rename = "userId")]
    user_id: u64,
}

// Existing payloads `{"user_id": 7}` now FAIL:
//   missing field `userId`
```

### Why it fails

`rename` replaces the wire name. Old payloads have the old name. Serde sees the old name as an unknown field (or as a missing required field, depending on the rest of the struct) and the deserialization fails.

### Fix

Keep the old name as an alias:

```rust
#[derive(Deserialize)]
struct User {
    #[serde(rename = "userId", alias = "user_id")]
    user_id: u64,
}
```

ALWAYS add `#[serde(alias = "old_name")]` when renaming a deserialize-facing field. `alias` is deserialize-only and stacks: multiple `alias` attributes are allowed.

---

## AP-6: `serde_json::Value` everywhere

### Bad

```rust
fn handle(input: &str) -> Result<(), Box<dyn std::error::Error>> {
    let v: serde_json::Value = serde_json::from_str(input)?;
    let user_id = v["user"]["id"].as_u64().ok_or("missing id")?;
    let name = v["user"]["name"].as_str().ok_or("missing name")?;
    let email = v["user"]["email"].as_str().ok_or("missing email")?;
    save_user(user_id, name, email);
    Ok(())
}
```

### Why it fails

- Compile-time type information is lost: typos in `["user"]["nme"]` return `Value::Null`, not a compile error.
- Validation is duplicated at every access: each `.as_str().ok_or(...)` branch is a separate failure path.
- Refactor cost: adding a field to the wire format means editing every `v["..."]` access.
- Performance: `Value` allocates a `BTreeMap` (or `IndexMap` with the `preserve_order` feature) for every object, even when the shape is known.

### Fix

Define a typed struct:

```rust
#[derive(Deserialize)]
struct Payload { user: User }
#[derive(Deserialize)]
struct User { id: u64, name: String, email: String }

fn handle(input: &str) -> Result<(), Box<dyn std::error::Error>> {
    let p: Payload = serde_json::from_str(input)?;
    save_user(p.user.id, &p.user.name, &p.user.email);
    Ok(())
}
```

Use `serde_json::Value` ONLY at trust boundaries (untyped third-party input, generic introspection tools, JSON-Patch / JSON-Pointer applications). NEVER use it as the working representation inside business logic.

---

## AP-7: `#[serde(skip)]` without `Default`

### Bad

```rust
#[derive(Deserialize)]
struct Cache {
    key: String,
    #[serde(skip)]
    computed: Computed,   // Computed: not Default
}
// error[E0277]: the trait `Default` is not implemented for `Computed`
```

### Why it fails

`skip` skips on *both* serialize and deserialize. On deserialize, the field still needs a value. Without `Default`, serde does not know how to populate it.

### Fix

Either implement `Default` for `Computed`, or provide a custom default function:

```rust
fn default_computed() -> Computed { Computed::new() }

#[derive(Deserialize)]
struct Cache {
    key: String,
    #[serde(skip, default = "default_computed")]
    computed: Computed,
}
```

Or use `skip_serializing` only (the field stays mandatory on deserialize):

```rust
#[derive(Serialize, Deserialize)]
struct Cache {
    key: String,
    #[serde(skip_serializing)]
    computed: Computed,
}
```

---

## AP-8: `from_reader` on a network socket

### Bad

```rust
let stream = TcpStream::connect("api.example.com:443")?;
let v: MyResponse = serde_json::from_reader(stream)?;
```

### Why it fails

`serde_json::from_reader` buffers internally and reads *to EOF*. On a network socket, EOF means the server closed the connection. If the server keeps the connection open for follow-up requests, `from_reader` blocks forever.

### Fix

Read into a `String` (with `Content-Length`) or use the streaming iterator if the wire format is concatenated JSON:

```rust
let mut buf = String::new();
stream.take(content_length).read_to_string(&mut buf)?;
let v: MyResponse = serde_json::from_str(&buf)?;
```

For ND-JSON streams:

```rust
for record in serde_json::Deserializer::from_reader(stream).into_iter::<Record>() {
    let r = record?;
    process(r);
}
```

NEVER call `from_reader` on an unbounded source. ALWAYS frame the input first.

---

## AP-9: `#[serde(tag = ...)]` on an enum with tuple variants

### Bad

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum Op {
    Set { key: String, value: String },
    Get(String),    // tuple variant
}
// error: #[serde(tag = "...")] cannot be used with tuple variants
```

### Why it fails

Internally tagged enums inject the tag *inside* the JSON object of each variant. A tuple variant serializes to a JSON array, which has no place for a `"type"` key.

### Fix

Promote tuple variants to struct variants:

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum Op {
    Set { key: String, value: String },
    Get { key: String },
}
```

Or switch to adjacently tagged, which keeps tuple variants intact:

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
enum Op {
    Set { key: String, value: String },
    Get(String),
}
// {"type":"Get","data":"k"}
```

---

## AP-10: Mixing the bincode 1.x and 2.x APIs

### Bad

```rust
// Cargo.toml: bincode = "2"
let v = bincode::serialize(&value)?;     // 1.x API; does not exist in 2.x
```

### Why it fails

Bincode 2.0 rewrote the API. The top-level `serialize` / `deserialize` functions are gone. Code copied from old tutorials or older Stack Overflow answers compiles only against bincode 1.x.

### Fix

Use the 2.x serde-bridge functions and an explicit config:

```rust
use bincode::config::standard;

let v = bincode::serde::encode_to_vec(&value, standard())?;
let (back, _read): (Value, usize) =
    bincode::serde::decode_from_slice(&v, standard())?;
```

Pin the bincode version explicitly in Cargo.toml (`bincode = "2"` or `bincode = "1"`) and never assume the top-level helpers exist. The chosen `config::*` (standard, legacy, custom) is part of the wire format: encoder and decoder MUST agree.

---

## Reference links

- Container attributes: <https://serde.rs/container-attrs.html>
- Field attributes: <https://serde.rs/field-attrs.html>
- Enum representations: <https://serde.rs/enum-representations.html>
- Lifetimes in deserialize: <https://serde.rs/lifetimes.html>
- `serde_json` API: <https://docs.rs/serde_json/latest/serde_json/>
- Bincode 2 migration: <https://docs.rs/bincode/latest/bincode/>
