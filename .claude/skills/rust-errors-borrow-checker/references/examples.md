# Borrow-checker errors: broken vs fixed examples

Side-by-side. The first block in each pair triggers the named error; the second compiles on Rust 1.85+, edition 2024.

---

## E0382: use of moved value

### Broken: move on assignment

```rust
fn main() {
    let s = String::from("hello");
    let t = s;             // s moved into t
    println!("{s}");       // E0382: borrow of moved value `s`
}
```

### Fixed A: borrow instead of move

```rust
fn main() {
    let s = String::from("hello");
    let t = &s;            // shared borrow, no move
    println!("{s} {t}");   // both valid
}
```

### Fixed B: clone (only when two owners genuinely needed)

```rust
fn main() {
    let s = String::from("hello");
    let t = s.clone();     // independent owned copy
    println!("{s} {t}");
}
```

### Broken: move into function

```rust
fn consume(v: Vec<i32>) -> usize { v.len() }

fn main() {
    let v = vec![1, 2, 3];
    let n = consume(v);    // v moved
    println!("{:?} {n}", v); // E0382
}
```

### Fixed: take a borrow in the function signature

```rust
fn count(v: &[i32]) -> usize { v.len() }

fn main() {
    let v = vec![1, 2, 3];
    let n = count(&v);     // borrow
    println!("{v:?} {n}"); // v still valid
}
```

### Broken: move in a loop

```rust
fn main() {
    let msg = String::from("tick");
    for _ in 0..3 {
        send(msg);         // E0382 on 2nd iteration: msg moved on 1st
    }
}
fn send(_s: String) {}
```

### Fixed A: borrow in the loop

```rust
fn main() {
    let msg = String::from("tick");
    for _ in 0..3 {
        send(&msg);        // borrow each iteration
    }
}
fn send(_s: &str) {}
```

### Fixed B: mem::take when the value must be moved out exactly once

```rust
use std::mem;

struct Job { payload: String }

impl Job {
    fn run(&mut self) -> String {
        mem::take(&mut self.payload) // payload becomes "", returns the old String
    }
}
```

### Broken: partial move out of a struct

```rust
struct Config { name: String, port: u16 }

fn main() {
    let cfg = Config { name: String::from("svc"), port: 8080 };
    let name = cfg.name;          // partial move
    println!("{}", cfg.name);     // E0382: use of moved value `cfg.name`
}
```

### Fixed: clone the moved-out field, or destructure fully

```rust
struct Config { name: String, port: u16 }

fn main() {
    let cfg = Config { name: String::from("svc"), port: 8080 };
    let name = cfg.name.clone();
    println!("{name} {}", cfg.name); // OK
    // or destructure: let Config { name, port } = cfg;
}
```

---

## E0502: mutable + immutable borrow at the same time

### Broken

```rust
fn main() {
    let mut v = vec![1, 2, 3];
    let first = &v[0];        // shared borrow
    v.push(4);                // E0502: cannot borrow `v` as mutable
    println!("{first}");
}
```

### Fixed A: tighten the shared-borrow scope (NLL)

```rust
fn main() {
    let mut v = vec![1, 2, 3];
    let first = &v[0];
    println!("{first}");      // last use of `first`; borrow ends here
    v.push(4);                // OK
}
```

### Fixed B: extract the read into an owned local

```rust
fn main() {
    let mut v = vec![1, 2, 3];
    let first = v[0];         // i32 is Copy, owned value, no live borrow
    v.push(4);                // OK
    println!("{first}");
}
```

### Broken: read a field while a method takes `&mut self`

```rust
struct Log { entries: Vec<String> }
impl Log {
    fn last(&self) -> &String { &self.entries[self.entries.len() - 1] }
    fn push(&mut self, s: String) { self.entries.push(s); }
}

fn main() {
    let mut log = Log { entries: vec![String::from("a")] };
    let prev = log.last();          // shared borrow of log
    log.push(String::from("b"));    // E0502
    println!("{prev}");
}
```

### Fixed: clone the read, or read after the write

```rust
fn main() {
    let mut log = Log { entries: vec![String::from("a")] };
    let prev = log.last().clone();   // owned copy; borrow ends
    log.push(String::from("b"));     // OK
    println!("{prev}");
}
```

---

## E0596: cannot borrow as mutable, not declared as mutable

### Broken: missing `mut` on a local

```rust
fn main() {
    let count = 0;
    let r = &mut count;   // E0596
    *r += 1;
}
```

### Fixed: add `mut`

```rust
fn main() {
    let mut count = 0;
    let r = &mut count;
    *r += 1;
}
```

### Broken: `&self` method mutates a field

```rust
struct Counter { n: u32 }
impl Counter {
    fn bump(&self) { self.n += 1; } // E0596
}
```

### Fixed A: change receiver to `&mut self`

```rust
struct Counter { n: u32 }
impl Counter {
    fn bump(&mut self) { self.n += 1; }
}
```

### Fixed B: interior mutability when the signature must stay `&self`

```rust
use std::cell::Cell;

trait Tick { fn tick(&self); } // trait fixes &self

struct Counter { n: Cell<u32> }
impl Tick for Counter {
    fn tick(&self) {
        self.n.set(self.n.get() + 1); // mutate through Cell, &self preserved
    }
}
```

### Broken: mutate through a `&` parameter

```rust
fn add_one(x: &i32) { *x += 1; } // E0596: x is &, not &mut
```

### Fixed: take `&mut`

```rust
fn add_one(x: &mut i32) { *x += 1; }
```

---

## E0499: multiple mutable borrows

### Broken

```rust
fn main() {
    let mut data = vec![1, 2, 3];
    let a = &mut data;
    let b = &mut data;   // E0499
    a.push(4);
    b.push(5);
}
```

### Fixed A: scope-tighten so the first `&mut` ends first

```rust
fn main() {
    let mut data = vec![1, 2, 3];
    { let a = &mut data; a.push(4); } // first borrow ends
    let b = &mut data;
    b.push(5);
}
```

### Fixed B: split borrow across distinct struct fields

```rust
struct Buffers { input: Vec<u8>, output: Vec<u8> }

fn main() {
    let mut b = Buffers { input: vec![], output: vec![] };
    let inp = &mut b.input;    // disjoint path
    let out = &mut b.output;   // disjoint path; both OK
    inp.push(1);
    out.push(2);
}
```

### Broken: two `&mut` into one Vec

```rust
fn main() {
    let mut v = vec![10, 20, 30, 40];
    let lo = &mut v[0];
    let hi = &mut v[3];   // E0499: indexing borrows the whole Vec
    *lo += 1;
    *hi += 1;
}
```

### Fixed: split_at_mut for disjoint subslices

```rust
fn main() {
    let mut v = vec![10, 20, 30, 40];
    let (left, right) = v.split_at_mut(2);
    left[0] += 1;
    right[1] += 1;
}
```

### Broken: two `&mut` field getters

```rust
struct Pair { a: i32, b: i32 }
impl Pair {
    fn a_mut(&mut self) -> &mut i32 { &mut self.a }
    fn b_mut(&mut self) -> &mut i32 { &mut self.b }
}

fn main() {
    let mut p = Pair { a: 1, b: 2 };
    let ra = p.a_mut();
    let rb = p.b_mut();  // E0499: each getter borrows all of *self
    *ra += 1;
    *rb += 1;
}
```

### Fixed: one method returning both, so the compiler sees disjoint paths

```rust
impl Pair {
    fn both_mut(&mut self) -> (&mut i32, &mut i32) {
        (&mut self.a, &mut self.b)
    }
}

fn main() {
    let mut p = Pair { a: 1, b: 2 };
    let (ra, rb) = p.both_mut();
    *ra += 1;
    *rb += 1;
}
```

---

## E0500: closure borrow conflict

### Broken

```rust
fn main() {
    let mut value = 10;
    let reader = &value;                 // shared borrow
    let mut writer = || value += 1;      // E0500: closure wants unique access
    writer();
    println!("{reader}");
}
```

### Fixed A: drop the conflicting borrow before the closure

```rust
fn main() {
    let mut value = 10;
    let reader = &value;
    println!("{reader}");                // shared borrow ends here
    let mut writer = || value += 1;
    writer();
}
```

### Fixed B: move closure when it should own the data

```rust
fn main() {
    let data = vec![1, 2, 3];
    let printer = move || println!("{data:?}"); // closure owns data
    printer();
}
```

### Broken: closure captures whole struct

```rust
struct S { a: i32, b: i32 }

fn main() {
    let mut s = S { a: 0, b: 0 };
    let read_b = &s.b;
    let mut inc = || s.a += 1;   // before disjoint capture this conflicts
    inc();
    println!("{read_b}");
}
```

### Fixed: disjoint closure captures (edition 2021+ behaviour)

Since edition 2021 closures capture individual fields, so the above already compiles when only `s.a` is used in the closure and only `s.b` outside. If the closure touches both fields, split the work:

```rust
fn main() {
    let mut s = S { a: 0, b: 0 };
    {
        let read_b = s.b;        // Copy out; no live borrow
        let mut inc = || s.a += 1;
        inc();
        println!("{read_b}");
    }
}
```

---

## E0716: temporary value dropped while borrowed

### Broken

```rust
fn make() -> String { String::from("data") }
fn first_char(s: &str) -> &str { &s[0..1] }

fn main() {
    let c = first_char(&make()); // temporary from make() dies after this line
    println!("{c}");             // E0716
}
```

### Fixed A: let-bind the temporary

```rust
fn main() {
    let s = make();              // owns the String for the whole block
    let c = first_char(&s);
    println!("{c}");
}
```

### Fixed B: bind via &expr (temporary lifetime extension)

```rust
fn main() {
    let s = &make();             // temporary extended to block scope
    let c = first_char(s);
    println!("{c}");
}
```

### Broken: borrow of a temporary in a match scrutinee

```rust
fn config() -> Vec<i32> { vec![1, 2, 3] }

fn main() {
    let r = match config().first() { // temporary Vec dropped at end of statement
        Some(x) => x,
        None => &0,
    };
    println!("{r}");                 // E0716
}
```

### Fixed: bind the Vec first

```rust
fn main() {
    let cfg = config();
    let r = match cfg.first() {
        Some(x) => x,
        None => &0,
    };
    println!("{r}");
}
```

---

## Realistic combined example: builder that returns borrows

### Broken

```rust
struct Report { lines: Vec<String> }

impl Report {
    fn add(&mut self, line: String) { self.lines.push(line); }
    fn header(&self) -> &String { &self.lines[0] }
}

fn main() {
    let mut r = Report { lines: vec![String::from("title")] };
    let h = r.header();                   // shared borrow
    r.add(String::from("body"));          // E0502
    println!("{h}");
}
```

### Fixed: read after write, or snapshot the header

```rust
fn main() {
    let mut r = Report { lines: vec![String::from("title")] };
    r.add(String::from("body"));          // mutate first
    let h = r.header();                   // borrow after mutation
    println!("{h}");
}
```
