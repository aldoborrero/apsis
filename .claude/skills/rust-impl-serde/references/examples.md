# Examples: serde patterns end-to-end

Runnable patterns. Every snippet compiles with `serde = { version = "1", features = ["derive"] }` plus the format-specific crate.

---

## 1. Round-trip a struct through JSON

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Point { x: i32, y: i32 }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let p = Point { x: 1, y: 2 };
    let json = serde_json::to_string(&p)?;
    assert_eq!(json, r#"{"x":1,"y":2}"#);

    let back: Point = serde_json::from_str(&json)?;
    assert_eq!(back, p);
    Ok(())
}
```

---

## 2. `rename_all` for a JavaScript-friendly API

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateUser {
    first_name: String,
    last_name: String,
    email_address: String,
}

// JSON in / out: {"firstName": "...", "lastName": "...", "emailAddress": "..."}
```

---

## 3. Schema migration: rename a field, accept the old name

```rust
#[derive(Deserialize)]
struct User {
    #[serde(rename = "userId", alias = "user_id", alias = "id")]
    user_id: u64,
    name: String,
}

let from_v1: User = serde_json::from_str(r#"{"id":7,"name":"a"}"#)?;
let from_v2: User = serde_json::from_str(r#"{"user_id":7,"name":"a"}"#)?;
let from_v3: User = serde_json::from_str(r#"{"userId":7,"name":"a"}"#)?;
// All three succeed; all three produce User { user_id: 7, .. }.
```

---

## 4. Partial-update payload with `skip_serializing_if`

```rust
#[derive(Serialize, Default)]
struct UserPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    age: Option<u32>,
}

let p = UserPatch { name: Some("Alice".into()), ..Default::default() };
let s = serde_json::to_string(&p)?;
assert_eq!(s, r#"{"name":"Alice"}"#);  // email and age are absent, not null
```

---

## 5. Strict config with `deny_unknown_fields`

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    port: u16,
    host: String,
}

let bad = r#"{"port":8080,"host":"localhost","prot":"http"}"#;
// "prot" is a typo of "host"; deserialize returns an error pointing to the unknown field.
let err = serde_json::from_str::<Config>(bad).unwrap_err();
assert!(err.to_string().contains("unknown field `prot`"));
```

---

## 6. Default values from a function

```rust
fn default_port() -> u16 { 8080 }

#[derive(Deserialize)]
struct ServerCfg {
    #[serde(default = "default_port")]
    port: u16,
    host: String,
}

let cfg: ServerCfg = serde_json::from_str(r#"{"host":"localhost"}"#)?;
assert_eq!(cfg.port, 8080);
```

---

## 7. `#[serde(default)]` on the whole struct

```rust
#[derive(Deserialize, Default)]
#[serde(default)]
struct AppCfg {
    debug: bool,
    workers: usize,
    log_path: String,
}

let cfg: AppCfg = serde_json::from_str("{}")?;
// All three fields take their Default values.
```

---

## 8. Flatten a sub-struct into the parent shape

```rust
#[derive(Serialize, Deserialize)]
struct Page<T> {
    items: Vec<T>,
    #[serde(flatten)]
    pagination: Pagination,
}

#[derive(Serialize, Deserialize)]
struct Pagination { page: u32, per_page: u32, total: u32 }

let body = Page {
    items: vec![1u32, 2, 3],
    pagination: Pagination { page: 1, per_page: 10, total: 100 },
};
let s = serde_json::to_string(&body)?;
assert_eq!(s, r#"{"items":[1,2,3],"page":1,"per_page":10,"total":100}"#);
```

---

## 9. Catch-all extra fields with a `HashMap`

```rust
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
struct Loose {
    id: u64,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

let v: Loose = serde_json::from_str(r#"{"id":1,"foo":"bar","baz":42}"#)?;
assert_eq!(v.extra["foo"], "bar");
assert_eq!(v.extra["baz"], 42);
```

NEVER add `#[serde(deny_unknown_fields)]` to a struct that contains a `#[serde(flatten)]` field. The two attributes are mutually exclusive on the same parent.

---

## 10. Enum: externally tagged (default)

```rust
#[derive(Serialize, Deserialize)]
enum Message {
    Quit,
    Move { x: i32, y: i32 },
    Write(String),
    ChangeColor(i32, i32, i32),
}

assert_eq!(serde_json::to_string(&Message::Quit)?, r#""Quit""#);
assert_eq!(serde_json::to_string(&Message::Move { x: 1, y: 2 })?, r#"{"Move":{"x":1,"y":2}}"#);
assert_eq!(serde_json::to_string(&Message::Write("hi".into()))?, r#"{"Write":"hi"}"#);
assert_eq!(serde_json::to_string(&Message::ChangeColor(1,2,3))?, r#"{"ChangeColor":[1,2,3]}"#);
```

---

## 11. Enum: internally tagged

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum Event {
    Login { user_id: u64 },
    Logout { user_id: u64 },
    Heartbeat,
}

assert_eq!(
    serde_json::to_string(&Event::Login { user_id: 7 })?,
    r#"{"type":"Login","user_id":7}"#,
);
assert_eq!(serde_json::to_string(&Event::Heartbeat)?, r#"{"type":"Heartbeat"}"#);
```

Tuple variants (`ChangeColor(i32, i32, i32)`) are NOT allowed under `tag = "..."`. Replace with a struct variant or switch to adjacently tagged.

---

## 12. Enum: adjacently tagged

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
enum Action {
    Click { x: i32, y: i32 },
    Type(String),
    Submit,
}

assert_eq!(
    serde_json::to_string(&Action::Click { x: 1, y: 2 })?,
    r#"{"t":"Click","c":{"x":1,"y":2}}"#,
);
assert_eq!(
    serde_json::to_string(&Action::Type("hello".into()))?,
    r#"{"t":"Type","c":"hello"}"#,
);
assert_eq!(serde_json::to_string(&Action::Submit)?, r#"{"t":"Submit"}"#);
```

---

## 13. Enum: untagged

```rust
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum StringOrInt {
    String(String),
    Int(i64),
}

let s: StringOrInt = serde_json::from_str(r#""hello""#)?;   // String variant
let n: StringOrInt = serde_json::from_str(r#"42"#)?;        // Int variant
```

ALWAYS verify the variants are *structurally* distinct. If two variants could both match `42`, the first one wins; the second never deserializes.

---

## 14. Zero-copy with `#[serde(borrow)]`

```rust
use std::borrow::Cow;

#[derive(Deserialize)]
struct Header<'a> {
    #[serde(borrow)]
    name: &'a str,
    #[serde(borrow)]
    value: Cow<'a, str>,
}

let raw = r#"{"name":"Content-Type","value":"text/plain"}"#;
let h: Header = serde_json::from_str(raw)?;
// h.name points into raw (no allocation).
// h.value is Cow::Borrowed unless the JSON contained escape sequences requiring unescape.
```

Without `#[serde(borrow)]`: error `implementation of Deserialize is not general enough`.

---

## 15. Newtype with `transparent`

```rust
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct EmailAddress(String);

let e = EmailAddress("a@b.com".into());
assert_eq!(serde_json::to_string(&e)?, r#""a@b.com""#);  // not {"0":"a@b.com"}
```

For a *tuple* newtype `struct EmailAddress(String);` serde defaults to transparent already; for a *named* newtype `struct EmailAddress { addr: String }` the attribute is required.

---

## 16. Custom `with` module for chrono-free Unix timestamps

```rust
mod unix_ts {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn serialize<S: Serializer>(t: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        let secs = t.duration_since(UNIX_EPOCH)
            .map_err(serde::ser::Error::custom)?
            .as_secs();
        s.serialize_u64(secs)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
        let secs = u64::deserialize(d)?;
        Ok(UNIX_EPOCH + std::time::Duration::from_secs(secs))
    }
}

#[derive(Serialize, Deserialize)]
struct Event {
    name: String,
    #[serde(with = "unix_ts")]
    occurred_at: std::time::SystemTime,
}
```

---

## 17. TOML configuration

```rust
// Cargo.toml: toml = "0.8"
#[derive(Deserialize)]
struct Cfg {
    server: ServerCfg,
    database: DbCfg,
}
#[derive(Deserialize)]
struct ServerCfg { host: String, port: u16 }
#[derive(Deserialize)]
struct DbCfg { url: String }

let raw = r#"
[server]
host = "127.0.0.1"
port = 8080

[database]
url = "postgres://..."
"#;
let cfg: Cfg = toml::from_str(raw)?;
```

---

## 18. Bincode for Rust-to-Rust on-disk format

```rust
// Cargo.toml: bincode = "2"
let p = Point { x: 1, y: 2 };
let bytes = bincode::serde::encode_to_vec(&p, bincode::config::standard())?;
let (back, _read): (Point, usize) =
    bincode::serde::decode_from_slice(&bytes, bincode::config::standard())?;
assert_eq!(back, p);
```

NEVER use bincode for cross-language wire formats: the encoding is implementation-defined and changes between bincode 1.x and 2.x.

---

## 19. Streaming JSON: one record at a time

```rust
use std::io::BufReader;
use std::fs::File;

let f = BufReader::new(File::open("events.ndjson")?);
let de = serde_json::Deserializer::from_reader(f);
for record in de.into_iter::<Event>() {
    let event = record?;
    handle(&event);
}
```

Use this pattern for ND-JSON (one JSON value per line) or for any concatenated-JSON stream. NEVER call `serde_json::from_reader` on the same input: it tries to consume the entire stream as one value.

---

## 20. Dynamic JSON with `Value` and back

```rust
let mut v: serde_json::Value = serde_json::from_str(r#"{"a":1,"b":[2,3]}"#)?;
v["a"] = serde_json::json!(42);
v["c"] = serde_json::json!("new");

let back = serde_json::to_string(&v)?;
// back: {"a":42,"b":[2,3],"c":"new"}
```

Use `Value` only at trust boundaries. Convert into typed structs as soon as the shape is known: `let typed: MyStruct = serde_json::from_value(v)?;`.
