# Crust — Technical Design

## One-Liner

Rust for the rest of us. Same syntax, same binary, but the compiler doesn't yell until you're ready.

---

## Architecture

```
 .crust source (Rust syntax)
     │
     ▼
┌─────────┐     ┌──────────┐     ┌─────────┐     ┌──────────┐
│  Parse   │────▶│  Desugar │────▶│  Check   │────▶│  Compile │
│  (Rust)  │     │  (level) │     │  (level) │     │  (rustc) │
└─────────┘     └──────────┘     └─────────┘     └──────────┘
```

1. **Parse** — full Rust grammar, hand-written recursive descent parser
2. **Desugar** — insert implicit clones, auto-derives, type coercions based on strictness level
3. **Check** — apply only the checks enabled at the current level
4. **Compile** — emit `.rs`, invoke `rustc -C opt-level=2` for native binary

`crust run` interprets directly via tree-walk evaluator.  
`crust build` emits Rust source and compiles to native binary.  
`crust build --emit-rs` also saves the intermediate `.rs` file for inspection.

---

## Strictness Levels

The core innovation. Each level enables more of rustc's checks:

### Level 0: Explore (default)

No borrow checker. No lifetime annotations. Implicit `Clone` on every move at codegen. Auto-derive `Clone, Debug, PartialEq` on user types (merged with author-supplied derives). Implicit numeric coercion (`i32` ↔ `i64` etc.) in the interpreter.

**Goal:** A Python developer can write Rust syntax and have it run on the first try. As of v0.2, all 13 bundled examples build via `crust build` and produce byte-identical stdout to `crust run`.

**What Crust does behind the scenes:**

| rustc error | Crust Level 0 behavior | Status |
|-------------|------------------------|--------|
| E0277 — Missing trait impl | Auto-derive `Clone, Debug, PartialEq`; auto-`T: Clone` bound on generics | Implemented |
| E0382 — Use after move | `.iter().cloned()` shim + per-closure `*p` strip-down based on iter signature | Implemented |
| E0308 — Type mismatch | Numeric coercion in the interpreter; integer literal defaulting | Implemented |
| E0282 — Can't infer type | Turbofish `::<T1, T2>` round-trips through codegen | Implemented |
| E0106 — Missing lifetime | Capture explicit `'lt`; bare `&T` returns with no input refs auto-promote to `&'static T` | Implemented |
| E0004 — Non-exhaustive `match` | `if let` lowering always emits a wildcard arm | Implemented |
| E0005 — Refutable pattern in `let` | Auto-inject `else { unreachable!() }` at Level <Ship | Implemented |
| E0599 — Method not found | Auto-resolve common stdlib methods | Partial — no fuzzy match |
| E0432 / E0425 — Unresolved import / name | Auto-import + fuzzy match | Not implemented (open) |

### Level 1: Develop

Adds (warning, doesn't block): shadow detection, panic-site warnings (`unwrap`, `expect`, `[idx]`, division-by-zero, `panic!`/`unreachable!`/`todo!`), arithmetic-overflow warnings, structural type-mismatch warnings, unsupported-feature warnings (`impl Trait`, explicit non-`'_` lifetimes, unknown macros, concurrency primitives, width-sensitive integer methods).

### Level 2: Harden

Same warnings as Develop today. Borrow-checker activation specifically remains a follow-on (would require building real lifetime/scope analysis on top of the AST).

### Level 3: Ship

Drops the auto-derives, drops the `.iter().cloned()` shim, drops the `force_pub` inside modules. Swaps `rustc` for `clippy-driver` with `-Dwarnings`, so any clippy lint is a hard build error (falls back to plain rustc with a one-line warning if `clippy-driver` isn't on PATH).

### Level 4: Prove

Extracts `#[requires]`/`#[ensures]`/`#[invariant]` into verification conditions. Discharges via z3 with typed sorts (Int/Real/Bool, with fallback comments for unmodelled types). Declares `result` with the function's actual return sort. Reports counter-examples by parsing z3 `(get-model)` output (e.g. `x=1, result=0`). Lowers bare `+`/`-`/`*` to `checked_*().expect("arithmetic overflow")` at codegen so overflow is explicit. Emits Coq (`.v`) and Lean 4 (`.lean`) skeletons where the function is an uninterpreted `Parameter`/`axiom` and the contract is a theorem with `admit`/`sorry` — the skeletons load into `coqc` and parse in Lean.

Real body-tied symbolic execution (so the SMT layer can prove the theorem rather than treat the body as uninterpreted) is tracked in `crust-v8b`.

---

## Parser

Hand-written recursive descent. No dependency on `syn` or proc-macro infrastructure.

Targets a Rust-2021 subset: items (fn, struct, enum, impl, trait, mod, const, type, use), expressions, all common pattern forms (slice with `..`, `@` bindings, range patterns, or-patterns, struct/tuple destructuring), generic parameter lists, explicit lifetimes on references, the `?` operator, `let-else`, `if let`, `while let`. Generic *bounds* (`T: Foo + Bar`) are silently consumed but their effects on dispatch are not modelled. Const generics, where-clauses, and trait associated types are skipped.

Crust-specific attributes Crust understands:
- `#[requires(pred)]`, `#[ensures(pred)]`, `#[invariant(pred)]` — contracts.
- `#[pure]` — assert no side-effects, checked at Level 4.
- Anything else passes through to rustc verbatim.

The parser produces a typed AST that feeds both the interpreter (`crust run`) and the code generator (`crust build`).

---

## Interpreter (crust run)

Tree-walk evaluator over the AST. Supports:

- Variables, functions, closures (capturing, `move`, returning closures)
- Control flow: if/else, match, for, while, loop, break/continue with labels, return
- Structs, enums (incl. tuple/struct variants), impl blocks, methods, default trait methods
- Traits with dynamic dispatch via `dyn Trait`
- Generics (type-erased at the interpreter — Crust's `>` is value-polymorphic so `fn max<T: Ord>` runs without monomorphisation; codegen still emits real generics for rustc to monomorphise)
- Inline modules (`mod NAME { items }`) with qualified-call resolution (`outer::inner::ident`)
- Standard library: `Vec`, `HashMap`, `HashSet`, `BTreeMap`, `BTreeSet` (dedicated sorted-and-deduped `Value::SortedSet` variant), `VecDeque`, `String`, `Option`, `Result`, `Range`, iterator adapters
- Built-in macros: `println!`, `print!`, `format!`, `vec!`, `assert!`/`assert_eq!`/`assert_ne!`, `dbg!`, `write!`/`writeln!`, `panic!`, `todo!`/`unimplemented!`/`unreachable!`. Other macros pass through to rustc at codegen time with a Develop+ warning.

At Level 0, the interpreter handles ownership by implicit cloning — every value is reference-counted internally. References (`&x`, `*y`, `Rc`, `Arc`, `Cell`, `RefCell`) are identity at the interpreter level; a real reference/aliasing model is `crust-0ku`.

Integer types collapse to `i64`; widths and signedness are not preserved (`crust-6yj`). Width-sensitive methods emit a Develop+ warning instead of silently producing wrong numbers.

---

## Code Generator (crust build)

Emits valid Rust source from the Crust AST, then invokes `rustc` (or `clippy-driver` at Ship). Edition 2021. Per-process temp file so concurrent invocations don't race.

At Level <Ship the generated Rust includes:
- `#![allow(unused_imports, unused_variables, unused_mut, unused_parens, dead_code)]` header and a stable `use std::collections::HashMap;` so simple programs don't trigger noise warnings
- `#[derive(Clone, Debug, PartialEq)]` on user structs/enums, merged with author-supplied derives
- Auto `T: Clone` bound on every generic parameter introduced at struct/enum/fn/impl/trait
- `.iter().cloned()` injected at iterator chain roots, paired with per-closure `*p` strip-down for the closure args of methods that take `Self::Item` by value (`map`, `fold`, `for_each`, `flat_map`, `filter_map`, `scan`, `any`, `all`, `position`); reference-taking methods (`filter`, `find`, `take_while`, `skip_while`, `inspect`, `max_by`, `min_by`, `max_by_key`, `min_by_key`) keep `*p` as written
- `&'static T` promotion on bare `&T` returns when no input ref is available for elision
- Range expressions parenthesised in receiver position (`(1..=10).sum()` not `1..=10.sum()`)
- `if let` lowering always with a wildcard arm (avoids E0004); refutable `let` patterns auto-inject `else { unreachable!() }` (avoids E0005)
- `mod NAME { items }` recursively emitted; items inside force-`pub` so callers can reach them (Crust doesn't track per-item visibility yet — `crust-1x4`)
- Forwarded `--extern NAME=PATH` and `-L PATH` flags for users with precompiled rlibs
- Cargo.toml detection: emits a one-line note pointing at the cargo workflow only when the program imports a non-std crate

At Level 4, additionally lowers bare `+`/`-`/`*` to `checked_*().expect("arithmetic overflow")` so overflow is explicit.

At Level Ship+, drops the auto-derives, drops the `force_pub` inside modules, drops `.iter().cloned()`, swaps in `clippy-driver -Dwarnings` so any clippy lint is a hard build error.

---

## Ownership Strategy by Level

| Level | Move semantics | Borrowing | Lifetimes |
|-------|---------------|-----------|-----------|
| 0 | Implicit clone on every move | Not required | Fully elided |
| 1 | Clone with warnings | Suggested | Elided with hints |
| 2 | Must be explicit | Required | Must annotate |
| 3 | Full Rust semantics | Full Rust semantics | Full Rust semantics |

The key insight: implicit clone at Level 0 is **correct** — it's just not zero-cost. A cloned `Vec` still produces the right answer. Performance optimization (moving to borrows) is what Levels 1-3 teach, incrementally.

---

## Verification (`--strict=4`)

Contract extraction in `src/contracts.rs`: walk attrs on each `FnDef`,
emit a `VerifCondition` per `#[requires]`/`#[ensures]`/`#[invariant]`. Each
VC carries the SMTLIB encoding of its predicate, the function's return
sort, and a status.

SMT discharge groups VCs by function so each function's preconditions
serve as assumptions for its postconditions:
- **`requires(P)`** — checks satisfiability of `P` (`assert P; check-sat`).
  `sat` means the precondition is consistent; `unsat` means contradictory.
- **`ensures(Q)`** — declares `result` with the function's actual return
  sort, asserts `(and pre ¬Q)`, expects `unsat` (no model where Q is
  violated). When `sat`, runs `(get-model)` and reports a counter-example
  like `(counter-example: x=1, result=0)`.

Without a body interpreter (`crust-v8b`), the function is treated as an
uninterpreted symbol — so `ensures` over `result` is bounded-soundness
only. The bead tracks the body-tied symbolic execution that would close
this gap.

Coq and Lean 4 emitters in `src/proofgen.rs` produce skeletons where the
function is a `Parameter` (Coq) / `axiom` (Lean), and the contract is a
theorem with `admit`/`sorry`. The skeletons load into `coqc` and parse
in Lean.

## Roadmap

| Version | Milestone | Status |
|---------|-----------|--------|
| **0.1** | Foundation — `crust run` and `crust build` pipeline | ✓ |
| **0.2** | Codegen end-to-end + Levels 1, 3, 4 enforced + verification surface | ✓ (this release) |
| **0.3** | Real reference/aliasing model in interpreter (`crust-0ku`); proof-mode body interpreter (`crust-v8b`) | open |
| **0.4** | Borrow-checker-equivalent activation at Level 2 (the only remaining "pure aspirational" piece of the dial) | open |
| **0.5** | Cargo workspace integration (`crust-ti9` follow-on); crate ecosystem; IDE LSP | open |
| **1.0** | Production — real-world Rust codebases, IDE integration, crate ecosystem | open |

---

## Prior Art

| Project | What it does | How Crust differs |
|---------|-------------|-------------------|
| **Rust (rustc)** | Full strictness from line one | Crust sequences the strictness |
| **Go** | Simple language, GC, fast compile | Crust targets Rust's performance, no GC |
| **Mojo** | Python-like syntax → fast binary | Crust IS Rust syntax, not a new language |
| **Carbon** | C++ successor by Google | Different lineage, Crust extends Rust's reach |
| **Vale** | Region-based memory, simpler ownership | New language; Crust keeps Rust compatibility |

Crust doesn't fork Rust. It's a **progressive disclosure frontend** for the same compiler, same ecosystem, same crates. A Crust developer is a Rust developer — they just started at Level 0.
