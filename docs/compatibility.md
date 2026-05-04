# Crust Rust Compatibility Contract

This is the authoritative description of what subset of Rust Crust accepts,
where Crust intentionally diverges from rustc semantics, and what is unsupported.
The parser, interpreter, code generator, and tests treat this document as
the source of truth.

When you find a divergence between this document and Crust's actual behaviour,
file a bd issue — the discrepancy is a bug in either the implementation or this
contract.

---

## 1. Strictness levels

Crust gates checks behind a `--strict=N` dial. Each level promises a strict
superset of the previous level.

| Level | Name    | Status today                                                      |
|-------|---------|-------------------------------------------------------------------|
| 0     | Explore | Default. Auto-derive `Clone, Debug, PartialEq` on user types; implicit `.clone()` injection on identifier-shaped argument and field positions; auto-deref for `.iter()` (compile output uses `iter().cloned()`). No analysis warnings. |
| 1     | Develop | Adds: panic-site warnings (`unwrap`, `expect`, `[idx]`, division-by-zero, `panic!`), arithmetic-overflow warnings, unsupported-feature warnings (impl Trait, explicit lifetimes, unknown macros, concurrency imports), and structural type-mismatch warnings. |
| 2     | Harden  | Same warnings as Develop today. Borrow-checker activation at this level is tracked in `crust-o3a`. |
| 3     | Ship    | Same diagnostics. Auto-derives are dropped — the developer controls the derive list, and codegen drops Crust's `iter().cloned()` shim. Full rustc parity beyond this is `crust-o3a`. |
| 4     | Prove   | Every Develop+ warning becomes a hard error. Codegen lowers bare `+`, `-`, `*` to `checked_*().expect("arithmetic overflow")`. `#[requires]` / `#[ensures]` are extracted; SMT discharge runs against z3 if `--verify` is passed. Wildcard `_` match arms become errors. |

`--llm-mode` is an orthogonal flag that adds: ban `unsafe`, ban `unwrap`/`expect`, ban `as` casts, ban `todo!`/`unimplemented!`/`unreachable!`. These are hard errors regardless of `--strict` level.

`--strict=4 --llm-mode` additionally requires explicit type annotations on every parameter and explicit return types.

---

## 2. Supported Rust syntax

Crust's parser is a hand-written recursive-descent over a Rust-2021 subset.
The following constructs are accepted and produce the documented behaviour.

### 2.1 Items

- `fn`, `async fn` — async fn at Levels 0–3 evaluates synchronously (the `.await` is a no-op); `--strict=4` rejects async fn (`crust-7ra`).
- `struct` — named-field, tuple, and unit forms. Author `#[derive(...)]` is preserved and merged with Crust's auto-derives at Level <Ship.
- `enum` — unit, tuple, and struct variants; explicit discriminants are parsed but not modelled.
- `impl` blocks — inherent and trait impls, with `&self` / `&mut self` / `self` receivers. Default trait methods are inherited when not overridden.
- `trait` — definition with default-method bodies. Supertrait bounds (`trait Foo: Bar`) are silently accepted but not modelled.
- `const`, `static` — module-level and impl-associated constants.
- `type` aliases.
- `use` — path imports. `use foo::*` glob import is supported only for enum variants.
- `mod NAME { items }` — inline modules are fully supported. `mod foo;` (file-based modules) is rejected with a clear diagnostic (`crust-rvq`).

### 2.2 Expressions

- All Rust operators (`+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, `||`, `&`, `|`, `^`, `<<`, `>>`, unary `-`/`!`).
- Function calls, method calls (with turbofish `::<T1, T2>`), field access, indexing.
- `if`/`else`, `match` (with guards, or-patterns, slice patterns, `@` bindings, range patterns), `for`, `while`, `loop`, labeled `break`/`continue`, `while let`, `if let`, `let-else`.
- Closures including `move` and tuple-pattern parameters.
- Range expressions (`a..b`, `a..=b`, `..b`, `a..`, `..`).
- `?` (try operator), `.await` (synchronous at Levels 0–3).
- `as` casts (flagged by `--llm-mode`).
- `unsafe { ... }` blocks (flagged by `--llm-mode` and by `#[pure]`).
- Macros: `println!`, `print!`, `eprintln!`, `eprint!`, `format!`, `vec![]`, `panic!`, `assert!`, `assert_eq!`, `assert_ne!`, `dbg!`, `write!`, `writeln!`, `todo!`, `unimplemented!`, `unreachable!`. All other macros are warned about (`crust-dfi`) and passed through to rustc verbatim by codegen.

### 2.3 Patterns

Wildcards, identifier bindings, literals, tuples, struct destructuring (with `..` rest), tuple-struct destructuring, or-patterns, range patterns (`'a'..='z'`), `&pat`, `name @ pat`, slice patterns `[a, b, rest @ .., z]`.

### 2.4 Types

- Primitives: `i8..i128`, `u8..u128`, `isize`, `usize`, `f32`, `f64`, `bool`, `char`, `str`, `String`. **Note**: the interpreter collapses every integer to `i64` and every float to `f64` (`crust-6yj`). Codegen preserves the original annotation, so `crust build` honours widths; `crust run` does not.
- Compound: tuples, arrays `[T; N]`, slices `&[T]`, `Vec<T>`.
- Standard library: `HashMap<K, V>`, `HashSet<T>`, `BTreeMap<K, V>`, `BTreeSet<T>`, `VecDeque<T>` (interpreter backs all of these with `Vec` or `HashMap`; ordering and uniqueness guarantees are approximate — `crust-kbu`).
- `Option<T>`, `Result<T, E>`, `Box<T>` (transparent at Level 0).
- References `&T`, `&mut T` — accepted by the parser but the interpreter elides them (treats `&x` as `x`). Lifetimes `'a` are parsed but ignored.
- `dyn Trait`, `impl Trait` — accepted; `impl Trait` collapses to a single named type at parse time (warned about at Develop+ — `crust-dfi`).
- `Fn(T) -> R`, `FnMut(T) -> R`, `FnOnce(T) -> R` — preserved as `Ty::FnPtr` and emitted faithfully.

---

## 3. Intentional divergences from rustc

These are **not** bugs. Crust deliberately deviates from rustc semantics in
the following ways to remain teaching-friendly at Level 0:

1. **No borrow checker at Levels 0–2.** Crust accepts code that rustc would reject for E0382 (use after move) at Levels 0–2. Implicit `.clone()` is injected at codegen time.
2. **Implicit clone on every `let` binding initialiser, fn-call argument, struct-field initialiser, and `.iter()` adapter chain** at Level <Ship. Programs are correct under value semantics but not zero-cost.
3. **Generics are type-erased in the interpreter.** `fn max<T: Ord>(a: T, b: T) -> T` runs at `crust run` because Crust's `>` is value-polymorphic; nothing is monomorphised. Codegen passes the generics to rustc which performs real monomorphisation.
4. **References are identity at runtime.** `&x` evaluates to `x`, `*y` evaluates to `y`. `Rc`, `Arc`, `Cell`, `RefCell` are treated as transparent containers (at the interpreter level only; codegen keeps them).
5. **All integers are `i64` in the interpreter.** Cast operations (`x as u32`) happen at codegen but are no-ops in `crust run`. Width-specific methods (`wrapping_*`, `checked_*`, `saturating_*`, `overflowing_*`, `leading_zeros`, `count_ones`, …) trigger an unsupported-feature warning at Develop+ since their interpreter results don't match rustc. `u128::MAX` / `i128::MAX` are clamped to `i64` boundaries because they don't fit Crust's value type. Stdlib constants `u8/u16/u32/u64::MAX`, `i8/i16/i32::MAX`, `usize/isize::MAX/MIN`, `f32/f64::MAX/MIN` are all defined; `u64::MAX` and `u128::MAX` are approximations (`crust-6yj`).
6. **HashMap iteration is sorted by key, not random.** Stdlib `std::collections::HashMap` randomises iteration order; Crust sorts by key for deterministic test output. `BTreeMap` iteration also lands in sorted order (matches rustc) by virtue of the same backing. Programs that rely on a specific HashMap iteration order are non-portable to rustc — don't write them.

7. **`BTreeSet` iterates in insertion order, not sorted order.** This *is* a divergence from rustc and is tracked by `crust-4ri`. Use `BTreeMap` (which iterates sorted) until that bead lands, or sort the result yourself.

8. **`VecDeque::push_front` is O(n)** because Crust backs the deque with a `Vec`. The visible semantics match rustc; only the asymptotic cost differs.

---

## 4. Unsupported features

These will either error at parse time, error at runtime, or fail to round-trip
through `crust build`. Tracked beads in parentheses are the home for each
gap; when the bead closes, this section will be updated.

### Parser-level rejection
- `mod foo;` (file-based modules) — rejected with a friendly diagnostic. Use inline `mod foo { ... }` for now (`crust-rvq`).
- Invalid `--strict=N` for N>4 — clamped to 4 with a warning.

### Diagnostics emitted at Develop+ (warning) / Prove (error)
- `impl Trait` parameter and return types (`crust-dfi`).
- Explicit non-`'_` lifetimes on parameters (`crust-dfi`).
- `async fn` at `--strict=4` (`crust-7ra`).
- Unknown macros — anything not in §2.2 (`crust-dfi`).
- Concurrency primitives: `Arc`, `Rc`, `Mutex`, `RwLock`, `mpsc::channel`, `std::thread`, `std::sync::atomic`. `crust run` errors at runtime; `crust build` passes them through to rustc (`crust-570`).

### Silently degraded — TODO file diagnostics
- Generic bounds (`<T: Foo + Bar>`) — `skip_generics`/`skip_where` silently drop them at parse time (`crust-1x4`).
- `where` clauses — silently dropped (`crust-1x4`).
- Const generics (`<const N: usize>`) — not parsed (`crust-1x4`).
- Trait associated types — `type Item = …` inside trait bodies is skipped (`crust-1x4`).
- File-based mod resolution and external crate dependencies (`crust-ti9`).

### Verification-mode partial
- `#[requires]` SMT discharge: satisfiability check only (no body interpretation; `crust-yi3`, `crust-7e8`).
- `#[ensures]` SMT discharge: best-effort with `result` declared as a free Int variable. Without a body interpreter, only constant postconditions and parameter-only postconditions are real proofs (`crust-v8b`).
- Coq / Lean proof skeletons emit each function as an uninterpreted `Parameter` / `axiom` and the contract theorem references it through `let result := f params in …`. Real proofs require body interpretation (`crust-v8b`).

### Codegen-level partial
- `crust build` round-trip is broken for programs that need real ownership-relaxation analysis (`crust-ovw`): `enums.crust`, `iterators.crust`, `option_result.crust`, `patterns.crust` all run under `crust run` but fail rustc compilation.

---

## 5. Strictness-level guarantees

The dial promise is monotonic: anything accepted at level N+1 is also
accepted at level N, and any diagnostic emitted at level N is also emitted
at level N+1 (possibly escalated to an error).

If you find a program that compiles at `--strict=4` but fails at
`--strict=0`, that's a contract violation. File a bug.

---

## 6. Versioning

This contract describes Crust v0.2. Future versions may broaden the
supported subset; they will not narrow it without a major version bump.
