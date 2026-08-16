# rust-syntax-lifetimes: examples

Compilable, edition-2024 examples for every pattern in SKILL.md. Each example is self-contained and can be pasted into `src/main.rs` (or wrapped in a test).

## Example 1: elision rule 2 (single input lifetime)

```rust
fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

fn main() {
    let s = String::from("hello world");
    assert_eq!(first_word(&s), "hello");
}
```

The compiler infers `fn first_word<'a>(s: &'a str) -> &'a str` because exactly one input lifetime exists.

## Example 2: elision rule 3 (method on `&self`)

```rust
struct Sentence(String);

impl Sentence {
    fn first(&self) -> &str {
        self.0.split_whitespace().next().unwrap_or("")
    }
}

fn main() {
    let s = Sentence(String::from("hello world"));
    assert_eq!(s.first(), "hello");
}
```

Output lifetime is taken from `&self` automatically.

## Example 3: elision failure (E0106)

```compile_fail
fn longest(x: &str, y: &str) -> &str {
    if x.len() > y.len() { x } else { y }
}
```

The compiler emits `error[E0106]: missing lifetime specifier`. The fix:

```rust
fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() { x } else { y }
}
```

## Example 4: struct with reference field

```rust
struct Excerpt<'a> {
    part: &'a str,
}

impl<'a> Excerpt<'a> {
    fn from_first_sentence(text: &'a str) -> Self {
        let first = text.split('.').next().unwrap_or("");
        Excerpt { part: first }
    }
}

fn main() {
    let novel = String::from("Call me Ishmael. Some years ago...");
    let excerpt = Excerpt::from_first_sentence(&novel);
    assert_eq!(excerpt.part, "Call me Ishmael");
}
```

## Example 5: `'static` two senses

```rust
fn store_static<T: 'static>(value: T) -> Box<T> {
    Box::new(value)
}

fn main() {
    let lit: &'static str = "literal";   // &'static T form
    let owned: String = String::from("owned");

    // String: 'static because it contains no non-'static borrows
    let boxed = store_static(owned);
    assert_eq!(*boxed, "owned");

    // &'static str also satisfies T: 'static
    let _boxed_ref = store_static(lit);

    let local = 7i32;
    let _r = &local;
    // store_static(_r); // ERROR: &'_ i32 does not satisfy 'static
}
```

## Example 6: HRTB

```rust
fn map_str<F>(s: &str, f: F) -> String
where
    F: for<'a> Fn(&'a str) -> &'a str,
{
    f(s).to_string()
}

fn main() {
    let trimmed = map_str("  hi  ", |x| x.trim());
    assert_eq!(trimmed, "hi");
}
```

The closure works for every input lifetime; without `for<'a>` the bound could not be expressed.

## Example 7: lifetime subtyping

```rust
struct Logger<'short, 'long: 'short> {
    name: &'short str,
    config_path: &'long str,
}

fn main() {
    let path = String::from("/etc/app.conf"); // longer
    let name = String::from("logger");        // shorter (but here same)
    let lg = Logger { name: &name, config_path: &path };
    assert_eq!(lg.name, "logger");
    assert_eq!(lg.config_path, "/etc/app.conf");
}
```

## Example 8: NLL (non-lexical lifetimes)

```rust
fn main() {
    let mut v = vec![1, 2, 3];
    let first = &v[0];
    println!("{first}");          // last use of `first`
    v.push(4);                    // OK under NLL
    assert_eq!(v.len(), 4);
}
```

## Example 9: variance (covariant `&'a T`)

```rust
fn takes_short<'a>(s: &'a str) -> &'a str { s }

fn main() {
    let lit: &'static str = "x";   // 'static is "longer than" any 'a
    let s: &str = takes_short(lit); // OK: &'static str is a subtype of &'a str
    assert_eq!(s, "x");
}
```

## Example 10: variance (invariant `&'a mut T`)

```compile_fail
fn assign<'a>(target: &mut &'a str, src: &'a str) {
    *target = src;
}

fn main() {
    let mut s: &'static str = "hi";
    let local = String::from("local");
    // &mut &'static str is INVARIANT in &'static str
    // so we cannot widen to &mut &'_local str
    assign(&mut s, &local); // ERROR: `local` does not live long enough
}
```

The fix is to NEVER attempt to assign a shorter borrow through a `&mut` to a longer-bound reference.

## Example 11: variance (contravariant `fn(&'a T)`)

```rust
type AnyStr = fn(&str);

fn takes_static(_: &'static str) { /* ... */ }

fn main() {
    // fn(&'a T) is contravariant in 'a:
    // fn(&'static T) is a SUBTYPE of fn(&'a T) for ANY shorter 'a
    let f: AnyStr = takes_static;
    f("anything");
}
```

## Example 12: edition-2024 RPIT capture

```rust
fn returns_iter<'a>(slice: &'a [i32]) -> impl Iterator<Item = &'a i32> {
    slice.iter()
}

fn main() {
    let v = vec![1, 2, 3];
    let sum: i32 = returns_iter(&v).sum();
    assert_eq!(sum, 6);
}
```

In edition 2024, `'a` is captured by default. In edition 2021, this signature would have required `+ Captures<'a>` or `+ 'a` workarounds.

## Example 13: edition-2024 RPIT opt-out with `+ use<>`

```rust
fn nothing_captured<'a, T>(_: &'a T) -> impl std::fmt::Debug + use<> {
    42_i32
}

fn main() {
    let v = vec![1];
    let d = nothing_captured(&v);
    // `v` can be dropped now because `d` captured nothing
    drop(v);
    println!("{d:?}");
}
```

## Example 14: lifetime in trait definition (GAT-adjacent)

```rust
trait Borrowable {
    fn borrow_part(&self) -> &str;
}

struct Greeting(String);

impl Borrowable for Greeting {
    fn borrow_part(&self) -> &str { &self.0 }
}

fn main() {
    let g = Greeting(String::from("hi"));
    assert_eq!(g.borrow_part(), "hi");
}
```

## Example 15: anonymous lifetime `'_`

```rust
struct Wrap<'a>(&'a str);

impl Wrap<'_> {
    fn get(&self) -> &str { self.0 }
}

fn main() {
    let s = String::from("hi");
    let w = Wrap(&s);
    assert_eq!(w.get(), "hi");
}
```

`Wrap<'_>` invokes elision; equivalent to `impl<'a> Wrap<'a>`.
