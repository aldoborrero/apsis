# Iterators and closures: complete method reference

All signatures sourced from the official documentation at:
- `std::iter::Iterator`: https://doc.rust-lang.org/std/iter/trait.Iterator.html
- `std::iter::IntoIterator`: https://doc.rust-lang.org/std/iter/trait.IntoIterator.html
- `std::ops::Fn` / `FnMut` / `FnOnce`: https://doc.rust-lang.org/std/ops/trait.Fn.html

---

## The `Iterator` trait

```rust
pub trait Iterator {
    type Item;

    // Required method
    fn next(&mut self) -> Option<Self::Item>;

    // Provided methods (selected)
    fn size_hint(&self) -> (usize, Option<usize>) { (0, None) }
    fn count(self) -> usize where Self: Sized;
    fn last(self) -> Option<Self::Item> where Self: Sized;
    fn nth(&mut self, n: usize) -> Option<Self::Item>;

    // Adapters returning new iterators (lazy)
    fn step_by(self, step: usize) -> StepBy<Self> where Self: Sized;
    fn chain<U>(self, other: U) -> Chain<Self, U::IntoIter>
        where Self: Sized, U: IntoIterator<Item = Self::Item>;
    fn zip<U>(self, other: U) -> Zip<Self, U::IntoIter>
        where Self: Sized, U: IntoIterator;
    fn map<B, F>(self, f: F) -> Map<Self, F>
        where Self: Sized, F: FnMut(Self::Item) -> B;
    fn for_each<F>(self, f: F)
        where Self: Sized, F: FnMut(Self::Item);
    fn filter<P>(self, predicate: P) -> Filter<Self, P>
        where Self: Sized, P: FnMut(&Self::Item) -> bool;
    fn filter_map<B, F>(self, f: F) -> FilterMap<Self, F>
        where Self: Sized, F: FnMut(Self::Item) -> Option<B>;
    fn enumerate(self) -> Enumerate<Self> where Self: Sized;
    fn peekable(self) -> Peekable<Self> where Self: Sized;
    fn skip_while<P>(self, predicate: P) -> SkipWhile<Self, P>
        where Self: Sized, P: FnMut(&Self::Item) -> bool;
    fn take_while<P>(self, predicate: P) -> TakeWhile<Self, P>
        where Self: Sized, P: FnMut(&Self::Item) -> bool;
    fn map_while<B, P>(self, predicate: P) -> MapWhile<Self, P>
        where Self: Sized, P: FnMut(Self::Item) -> Option<B>;
    fn skip(self, n: usize) -> Skip<Self> where Self: Sized;
    fn take(self, n: usize) -> Take<Self> where Self: Sized;
    fn scan<St, B, F>(self, initial_state: St, f: F) -> Scan<Self, St, F>
        where Self: Sized, F: FnMut(&mut St, Self::Item) -> Option<B>;
    fn flat_map<U, F>(self, f: F) -> FlatMap<Self, U, F>
        where Self: Sized, U: IntoIterator, F: FnMut(Self::Item) -> U;
    fn flatten(self) -> Flatten<Self>
        where Self: Sized, Self::Item: IntoIterator;
    fn fuse(self) -> Fuse<Self> where Self: Sized;
    fn inspect<F>(self, f: F) -> Inspect<Self, F>
        where Self: Sized, F: FnMut(&Self::Item);
    fn by_ref(&mut self) -> &mut Self where Self: Sized;
    fn rev(self) -> Rev<Self> where Self: Sized + DoubleEndedIterator;
    fn cloned<'a, T: 'a>(self) -> Cloned<Self>
        where Self: Sized + Iterator<Item = &'a T>, T: Clone;
    fn copied<'a, T: 'a>(self) -> Copied<Self>
        where Self: Sized + Iterator<Item = &'a T>, T: Copy;
    fn cycle(self) -> Cycle<Self> where Self: Sized + Clone;

    // Consumers (eager)
    fn collect<B: FromIterator<Self::Item>>(self) -> B where Self: Sized;
    fn try_collect<B>(&mut self) -> ChangeOutputType<Self::Item, B> // nightly
        where Self: Sized;
    fn fold<B, F>(self, init: B, f: F) -> B
        where Self: Sized, F: FnMut(B, Self::Item) -> B;
    fn reduce<F>(self, f: F) -> Option<Self::Item>
        where Self: Sized, F: FnMut(Self::Item, Self::Item) -> Self::Item;
    fn try_fold<B, F, R>(&mut self, init: B, f: F) -> R
        where F: FnMut(B, Self::Item) -> R, R: Try<Output = B>;
    fn all<F>(&mut self, f: F) -> bool where F: FnMut(Self::Item) -> bool;
    fn any<F>(&mut self, f: F) -> bool where F: FnMut(Self::Item) -> bool;
    fn find<P>(&mut self, predicate: P) -> Option<Self::Item>
        where P: FnMut(&Self::Item) -> bool;
    fn find_map<B, F>(&mut self, f: F) -> Option<B>
        where F: FnMut(Self::Item) -> Option<B>;
    fn position<P>(&mut self, predicate: P) -> Option<usize>
        where P: FnMut(Self::Item) -> bool;
    fn rposition<P>(&mut self, predicate: P) -> Option<usize>
        where Self: ExactSizeIterator + DoubleEndedIterator,
              P: FnMut(Self::Item) -> bool;
    fn max(self) -> Option<Self::Item> where Self: Sized, Self::Item: Ord;
    fn min(self) -> Option<Self::Item> where Self: Sized, Self::Item: Ord;
    fn max_by_key<B: Ord, F>(self, f: F) -> Option<Self::Item>
        where Self: Sized, F: FnMut(&Self::Item) -> B;
    fn min_by_key<B: Ord, F>(self, f: F) -> Option<Self::Item>
        where Self: Sized, F: FnMut(&Self::Item) -> B;
    fn max_by<F>(self, compare: F) -> Option<Self::Item>
        where Self: Sized, F: FnMut(&Self::Item, &Self::Item) -> Ordering;
    fn min_by<F>(self, compare: F) -> Option<Self::Item>
        where Self: Sized, F: FnMut(&Self::Item, &Self::Item) -> Ordering;
    fn sum<S>(self) -> S where Self: Sized, S: Sum<Self::Item>;
    fn product<P>(self) -> P where Self: Sized, P: Product<Self::Item>;
    fn cmp<I>(self, other: I) -> Ordering
        where I: IntoIterator<Item = Self::Item>, Self::Item: Ord, Self: Sized;
    fn partial_cmp<I>(self, other: I) -> Option<Ordering>
        where I: IntoIterator, Self::Item: PartialOrd<I::Item>, Self: Sized;
    fn eq<I>(self, other: I) -> bool
        where I: IntoIterator, Self::Item: PartialEq<I::Item>, Self: Sized;
    fn ne<I>(self, other: I) -> bool
        where I: IntoIterator, Self::Item: PartialEq<I::Item>, Self: Sized;
}
```

Lazy vs eager classification:

- Lazy (return another iterator, no work done yet): `map`, `filter`, `filter_map`, `take`, `skip`, `take_while`, `skip_while`, `map_while`, `enumerate`, `zip`, `chain`, `flat_map`, `flatten`, `peekable`, `cloned`, `copied`, `step_by`, `rev`, `cycle`, `fuse`, `inspect`, `scan`
- Eager (consume the iterator immediately): `collect`, `fold`, `reduce`, `for_each`, `count`, `last`, `nth`, `any`, `all`, `find`, `find_map`, `position`, `rposition`, `min`, `max`, `min_by`, `max_by`, `min_by_key`, `max_by_key`, `sum`, `product`, `cmp`, `partial_cmp`, `eq`, `ne`

---

## `IntoIterator` trait

```rust
pub trait IntoIterator {
    type Item;
    type IntoIter: Iterator<Item = Self::Item>;
    fn into_iter(self) -> Self::IntoIter;
}
```

Standard impls for `Vec<T>`:

- `impl<T> IntoIterator for Vec<T>` -> `Item = T` (consumes the Vec, yields owned items)
- `impl<'a, T> IntoIterator for &'a Vec<T>` -> `Item = &'a T` (yields shared refs)
- `impl<'a, T> IntoIterator for &'a mut Vec<T>` -> `Item = &'a mut T` (yields mutable refs)

A `for x in v` loop desugars to:

```rust
let mut __iter = IntoIterator::into_iter(v);
while let Some(x) = __iter.next() {
    // body
}
```

---

## Iterator-related sub-traits

```rust
// Iteration count is exact and known.
pub trait ExactSizeIterator: Iterator {
    fn len(&self) -> usize { /* derived from size_hint by default */ }
    fn is_empty(&self) -> bool { self.len() == 0 } // nightly
}

// Can iterate from the back.
pub trait DoubleEndedIterator: Iterator {
    fn next_back(&mut self) -> Option<Self::Item>;
    // provided: nth_back, try_rfold, rfold, rfind
}

// `next` returning None is permanent. Empty marker trait.
pub trait FusedIterator: Iterator {}
```

`DoubleEndedIterator` enables `.rev()`. `FusedIterator` enables optimisations in adapters that rely on the post-None contract.

---

## Closure-related traits (the `Fn` hierarchy)

```rust
// Most-permissive: callable multiple times, captures used immutably.
pub trait Fn<Args>: FnMut<Args> {
    extern "rust-call" fn call(&self, args: Args) -> Self::Output;
}

// Mid: callable multiple times, captures may be mutated.
pub trait FnMut<Args>: FnOnce<Args> {
    extern "rust-call" fn call_mut(&mut self, args: Args) -> Self::Output;
}

// Least-permissive: callable at least once, captures consumed.
pub trait FnOnce<Args> {
    type Output;
    extern "rust-call" fn call_once(self, args: Args) -> Self::Output;
}
```

These traits are unstable to implement manually; user code uses the parenthesised sugar:

- `Fn(A, B) -> R` desugars to `Fn<(A, B), Output = R>`
- `FnMut(A) -> R` desugars to `FnMut<(A,), Output = R>`
- `FnOnce() -> R` desugars to `FnOnce<(), Output = R>`

Auto-implementations:

- Every function item type implements `Fn`, `FnMut`, `FnOnce`
- Every function pointer type (`fn(A) -> R`) implements `Fn`, `FnMut`, `FnOnce`
- Every closure implements one or more of these, picked by the compiler from the body

Higher-Ranked Trait Bounds for closures:

```rust
fn run<F>(f: F)
where
    F: for<'a> Fn(&'a str) -> &'a str,
{ /* ... */ }
```

Required when a closure must work for **any** input lifetime, not a single inferred one.

---

## Async closure traits (Rust 1.85)

Stabilised in Rust 1.85 ([release notes](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)).

```rust
pub trait AsyncFnOnce<Args> {
    type Output;
    type CallOnceFuture: Future<Output = Self::Output>;
    fn async_call_once(self, args: Args) -> Self::CallOnceFuture;
}

pub trait AsyncFnMut<Args>: AsyncFnOnce<Args> {
    type CallRefFuture<'a>: Future<Output = Self::Output> where Self: 'a;
    fn async_call_mut<'a>(&'a mut self, args: Args) -> Self::CallRefFuture<'a>;
}

pub trait AsyncFn<Args>: AsyncFnMut<Args> {
    fn async_call<'a>(&'a self, args: Args) -> Self::CallRefFuture<'a>;
}
```

Sugar:

- `AsyncFn(A) -> R` desugars to `AsyncFn<(A,), Output = R>`
- Same for `AsyncFnMut` / `AsyncFnOnce`

None of the `AsyncFn` family is dyn-compatible. For dynamic dispatch, return `Pin<Box<dyn Future<Output = R> + Send>>` from a regular `Fn` closure.

---

## Standard iterator constructors

| Constructor | Signature | Yields |
|-------------|-----------|--------|
| `iter::empty::<T>()` | `fn empty() -> Empty<T>` | nothing |
| `iter::once(x)` | `fn once<T>(x: T) -> Once<T>` | exactly one `x` |
| `iter::repeat(x)` | `fn repeat<T: Clone>(x: T) -> Repeat<T>` | `x` forever |
| `iter::repeat_with(f)` | `fn repeat_with<F: FnMut() -> A>(f: F) -> RepeatWith<F>` | `f()` forever |
| `iter::successors(init, f)` | `fn successors<T, F: FnMut(&T) -> Option<T>>(first: Option<T>, succ: F) -> Successors<T, F>` | `init`, `f(&init)`, ... until `None` |
| `iter::from_fn(f)` | `fn from_fn<T, F: FnMut() -> Option<T>>(f: F) -> FromFn<F>` | whatever `f` produces |
| `iter::zip(a, b)` | `fn zip<A, B>(a: A, b: B) -> Zip<A::IntoIter, B::IntoIter>` | pairs |

Numeric ranges: `0..10`, `0..=9`, `(0..).step_by(2)`. All implement `Iterator`.

---

## `FromIterator` and `Extend`

```rust
pub trait FromIterator<A>: Sized {
    fn from_iter<T: IntoIterator<Item = A>>(iter: T) -> Self;
}

pub trait Extend<A> {
    fn extend<T: IntoIterator<Item = A>>(&mut self, iter: T);
}
```

`collect::<C>()` calls `C::from_iter(self)`. Implementing `FromIterator` for a custom collection makes `.collect()` work for it.

`Extend` lets `extend()`, `chain()` consumers, and other operations append iterator output to an existing collection.

Rust 1.85 extended `FromIterator` and `Extend` impls to 12-tuples, useful for collecting heterogeneous data structures.
