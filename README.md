# Crust — Rust for the rest of us

> *What if Rust didn't yell at you until you were ready?*

**Crust** is Rust without the learning cliff. Same syntax, same semantics, same binary — but the borrow checker, lifetime annotations, and type system complexity stay out of your way until you ask for them.

You write Rust. It runs. When you're ready for the compiler to get strict, you turn the dial.

```rust
fn main() {
    let data = vec![1, 2, 3, 4, 5];
    let sum = calculate_sum(data);
    println!("Sum of {data:?} is {sum}");  // moved? Crust handles it.
}

fn calculate_sum(nums: Vec<i32>) -> i32 {
    nums.iter().sum()
}
```

```bash
$ crust run main.crust
Sum of [1, 2, 3, 4, 5] is 15

$ crust build main.crust -o main
   Compiled crust v0.2.0
    Finished `release` profile [optimized]
      Binary: main

$ ./main
Sum of [1, 2, 3, 4, 5] is 15
```

No borrow checker errors. No lifetime annotations. No fighting the compiler for 45 minutes to print a list. Just Rust that works.

---

## The Problem

Every company wants to hire Rust developers. There aren't enough.

Rust is the most admired language eight years running. It produces the fastest, safest binaries of any modern language. Every systems team, every infrastructure org, every cloud vendor wants more Rust. But the talent pool is a puddle.

**25% of developers who try Rust give up** because it's "too intimidating, too hard to learn, or too complicated" (Rust Community Survey, 2017–2024).

JetBrains analyzed millions of builds and published the [top 10 compiler errors](https://blog.jetbrains.com/rust/2023/12/14/the-most-common-rust-compiler-errors-as-encountered-in-rustrover/) that kill adoption:

| Rank | Error | What it means | % of devs hit |
|------|-------|--------------|---------------|
| **1** | E0277 | Type doesn't implement required trait | **32%** |
| **2** | E0308 | Type mismatch | **30%** |
| **3** | E0599 | Method not found on type | **27.5%** |
| **4** | E0425 | Unresolved name | **20.5%** |
| **5** | E0433 | Undeclared module/crate | **17.5%** |
| **6** | E0382 | Use after move (ownership) | **17%** |
| **7** | E0432 | Unresolved import | **15.5%** |
| **8** | E0282 | Can't infer type | **13.5%** |
| **9** | E0061 | Wrong number of arguments | **13%** |
| **10** | E0412 | Type not in scope | **12%** |

The borrow checker (E0382) isn't even the top killer — it's **#6**. The entire type system is the wall. Developers hit errors on every axis — traits, types, ownership, imports — all at once, from line one, with no way to say "not yet."

Stanford researchers confirmed it ([Zeng & Crichton, 2018](https://arxiv.org/abs/1901.01001)): solutions to every common Rust pattern exist, but beginners can't find them because the compiler demands mastery before it allows progress.

**The result:** companies can't hire Rust devs, because the language won't let people become Rust devs.

---

## The Fix

Crust doesn't change Rust. It sequences the learning curve.

Every one of those top 10 errors has a reasonable default that a beginner doesn't need to understand yet. The list below is the **target design**; the
"Status" column tracks what's actually implemented today.

| Error | Crust Level 0 behaviour | Status |
|-------|-------------------------|--------|
| E0277 — Missing trait impl | Auto-derive `Clone`, `Debug`, `PartialEq` on user types; merge with author-supplied derives; auto-`T: Clone` bound on generics | Implemented |
| E0382 — Use after move | `.iter().cloned()` on iterator chains plus per-closure `*p` strip-down based on the std iter signature | Implemented |
| E0308 — Type mismatch | Safe numeric coercion in the interpreter; integer literal defaulting | Implemented (interpreter); codegen preserves width annotations |
| E0282 — Can't infer type | Turbofish (`::<T1, T2>`) round-trips end-to-end through codegen | Implemented |
| E0599 — Method not found | Auto-resolve common stdlib methods | Partial — no fuzzy match yet |
| E0432 — Unresolved import | Auto-import std prelude paths | **Not implemented** — non-std imports trigger a `--extern`/cargo note |
| E0425 — Unresolved name | Fuzzy match + suggest | **Not implemented** |
| E0106 — Missing lifetime | Capture explicit `'lt` annotations; bare `&T` returns with no input refs auto-promote to `&'static T` | Implemented |
| E0004 — Non-exhaustive match | `if let` lowering always emits a wildcard arm | Implemented |
| E0005 — Refutable pattern in let | Auto-inject `else { unreachable!() }` at Level <Ship | Implemented |

The code is still Rust. The generated binary is still produced by `rustc`. As of v0.2, **all 13 bundled example programs build via `crust build` and produce stdout byte-identical to `crust run`** — closed-loop tested. The remaining open beads are research-grade items (interpreter reference model, proof-mode body interpreter); see [Current Status](#current-status).

### The Strictness Dial

```
Level 0: Explore    — no borrow checker, implicit Clone, auto-derive, type coercion
                      "Python ease, Rust syntax"

Level 1: Develop    — warnings on moves, type mismatch hints, shadow detection
                      "The compiler is your mentor, not your drill sergeant"

Level 2: Harden     — borrow checker active, must annotate lifetimes, explicit types
                      "Training wheels off"

Level 3: Ship       — full rustc parity, cargo clippy clean, zero-cost abstractions
                      "This IS rustc"
```

> **Current state:**
> - **Level 0 (Explore)** — codegen end-to-end working; 13/13 examples build and
>   match `crust run` stdout byte-for-byte.
> - **Level 1 (Develop)** — adds shadow detection, panic-site warnings,
>   arithmetic-overflow warnings, unsupported-feature warnings (impl Trait,
>   explicit lifetimes, async fn at Prove, unknown macros, concurrency
>   primitives, width-sensitive integer methods).
> - **Level 2 (Harden)** — same warnings as Develop today. Borrow-checker
>   activation specifically remains a follow-on.
> - **Level 3 (Ship)** — drops auto-derives and the `.iter().cloned()` shim;
>   swaps `rustc` for `clippy-driver` with `-Dwarnings`, so any clippy lint is
>   a hard build error.
> - **Level 4 (Prove)** — extracts `#[requires]`/`#[ensures]` contracts,
>   discharges via z3 with typed sorts (Int/Real/Bool) and counter-example
>   reporting, lowers bare arithmetic to `checked_*().expect(...)`. Body-tied
>   verification (real symbolic execution) is tracked in `crust-v8b`.
>
> See [docs/compatibility.md](docs/compatibility.md) for the authoritative
> contract.

---

## Why This Works

**For developers:** You learn Rust by writing Rust — not by reading a 400-page book before you can print hello. Every concept arrives when you need it, explained by the compiler at your current level.

**For hiring managers:** Your Rust talent pool just became "anyone who can write code." Crust developers write real Rust syntax from day one. By the time they hit Level 2, they're mid-level Rust developers. You didn't train them — the tool did.

**For the Rust ecosystem:** More developers writing Rust means more crates, more libraries, more production deployments. Crust isn't a fork — it's a funnel.

---

## How It Works

```
 .crust source (Rust syntax)
     │
     ▼
┌─────────┐     ┌──────────┐     ┌─────────┐     ┌──────────┐
│  Parse   │────▶│  Desugar │────▶│  Check   │────▶│  Compile │
│  (Rust)  │     │  (level) │     │  (level) │     │  (rustc) │
└─────────┘     └──────────┘     └─────────┘     └──────────┘
```

1. **Parse** — full Rust grammar, hand-written recursive descent
2. **Desugar** — insert implicit clones, auto-derives, type coercions based on strictness level
3. **Check** — apply only the checks enabled at current level
4. **Compile** — emit `.rs`, invoke `rustc` for native binary

`crust run` interprets directly. `crust build` emits Rust and compiles. The intermediate `.rs` is always inspectable with `--emit-rs`.

---

## The Market

This isn't a language play. It's a **hiring play.**

- **3.5M** job postings mention Rust (2024, growing 30% YoY)
- **~3M** estimated Rust developers worldwide (vs. 50M Python, 17M JavaScript)
- **$158K** median Rust developer salary (highest of any language, Stack Overflow 2024)
- **25%** Rust attrition rate among learners

The supply/demand gap is the product. Every developer who bounces off `rustc` is a customer.

Crust doesn't compete with Rust. **Crust manufactures Rust developers.**

---

## Quick Start

Build and install from this repository (no crates.io publish yet):

```bash
make install                    # builds release and installs into /usr/local/bin

crust run hello.crust           # interpret + run
crust build hello.crust -o app  # compile a self-contained .crust file via rustc
crust build --emit-rs lib.crust # also write the generated .rs alongside
crust verify foo.crust --strict=4 --emit-proof   # JSON report + Coq/Lean
crust repl                      # interactive REPL with rustyline
```

`crust` accepts a single `.crust` file. Programs that import std are first-class.
For programs that need other crates, pass precompiled rlibs via `--extern` and
`-L`:

```bash
crust build app.crust \
  --extern serde=/path/to/libserde-HASH.rlib \
  -L /path/to/deps \
  -o app
```

A sibling `Cargo.toml` is detected and a one-line note suggests the cargo
workflow when (and only when) the program actually imports a non-std crate.
Full cargo workspace integration is the only real `crust-ti9` follow-on.

### Strictness flags

```bash
crust build foo.crust --strict=0   # default: implicit clones, auto-derive
crust build foo.crust --strict=1   # adds shadow detection + warnings
crust build foo.crust --strict=3   # invokes clippy-driver -Dwarnings
crust build foo.crust --strict=4   # contract extraction, checked arithmetic
crust build foo.crust --llm-mode   # bans unsafe, unwrap, todo!, as casts
```

## Development

Run `make check` before closing code changes. See [docs/testing.md](docs/testing.md)
for the TDD and coverage policy.

---

## Current Status

**v0.2.0** — Level 0 codegen end-to-end working; verification surface scaffolded.

- **200 tests pass** (148 unit + 52 integration), `cargo fmt --check` clean,
  `cargo clippy -D warnings` clean.
- **All 13 bundled examples** build via `crust build` and produce stdout
  byte-identical to `crust run` (verified by the differential-parity test).
- **Coverage**: regions 41.68%, lines 43.70%, functions 61.89%. CI ratchet at 40%.

Implemented:

- **All primitive types**: `i8`–`i128`, `u8`–`u128`, `isize`, `usize`, `f32`/`f64`, `bool`, `char`, `str`/`String`. Width-sensitive methods (`wrapping_*`, `checked_*`, `saturating_*`, `overflowing_*`, `leading_zeros`, …) emit a Develop+ warning since the interpreter collapses to `i64`.
- **Collections**: `Vec`, `HashMap`, `HashSet`, `BTreeMap`, `BTreeSet`, `VecDeque`. `HashMap` / `BTreeMap` / `BTreeSet` iterate in sorted order matching rustc; `BTreeSet` is backed by a dedicated `Value::SortedSet` variant that maintains the sorted-and-deduped invariant on insert.
- **Traits**: definition, implementation, default methods, `dyn Trait`, operator overloading (`Add`, `Mul`, `Neg`, etc.). `impl Trait` accepted with a Develop+ warning since the parser collapses bounds.
- **Closures**: capturing, `move`, `FnMut`, returning closures, higher-order functions. Iterator-chain closures get per-method `*p` strip-down so user code with explicit derefs round-trips through codegen.
- **Pattern matching**: destructuring, guards, or-patterns, slice patterns `[a, b, ..]`, range patterns, `@` bindings. Refutable patterns in `let` auto-inject `else { unreachable!() }` at Level <Ship.
- **Error handling**: `Result`/`Option` with `?` operator, combinators.
- **Iterators**: `map`, `filter`, `fold`, `flat_map`, `zip`, `enumerate`, `chain`, `scan`, `partition`, `unzip`, custom `Iterator` impls.
- **Control flow**: `for`, `while`, `loop`, labeled breaks, `while let`, `if let`, `let-else`.
- **Generics**: type-erased at the interpreter; codegen captures and re-emits `<T, U>` parameter lists with auto `T: Clone` bound at Level <Ship. Explicit lifetimes round-trip; bare-`&T` returns auto-promote to `&'static T`.
- **Modules**: inline `mod NAME { items }` (file-based `mod foo;` rejected with a clear diagnostic).
- **Macros**: `println!`, `print!`, `eprintln!`, `eprint!`, `format!`, `vec!`, `panic!`, `assert!`, `assert_eq!`, `assert_ne!`, `dbg!`, `write!`, `writeln!`, `todo!`, `unimplemented!`, `unreachable!`. Other macros pass through to `rustc` with a Develop+ warning.
- **Verification (`--strict=4`)**: extracts `#[requires]`/`#[ensures]`/`#[invariant]`, discharges via z3 with typed sorts, declares `result` with the function's actual return sort, reports counter-examples by parsing z3 `(get-model)` output. Lowers bare `+`/`-`/`*` to `checked_*().expect(…)` at codegen.
- **Proof skeletons**: `--emit-proof` writes `.v` (Coq) and `.lean` (Lean 4) files where each function is an uninterpreted `Parameter`/`axiom` and the contract is a theorem with `admit`/`sorry`. The skeletons load into `coqc` and parse in Lean.
- **`--llm-mode`**: hard-fails on `unsafe`, `unwrap`, `expect`, `as` casts, `todo!`/`unimplemented!`/`unreachable!`. At `--strict=4`, also requires explicit type annotations on every parameter and explicit return types.

### Open work (beads)

Two research-grade items remain. Both are multi-week designs in their own right; everything else from the v0.2 surface either landed or is documented as a tracked divergence in [docs/compatibility.md](docs/compatibility.md).

- `crust-0ku` (P1) — interpreter reference / aliasing / mutation model. Today `&x` is identity, `Rc`/`Arc`/`Cell`/`RefCell` are transparent. A real model would track borrow scopes and aliasing constraints.
- `crust-v8b` (P2) — proof-mode body interpreter. Today the SMT layer treats functions as uninterpreted; real soundness requires symbolic execution of the body so contracts are proved against actual semantics, not just consistency.

See [DESIGN.md](DESIGN.md) for the technical architecture and
[docs/compatibility.md](docs/compatibility.md) for the authoritative Rust
compatibility contract — the supported subset, intentional divergences from
rustc, and unsupported features tracked against beads.

---

## The Bet

The world doesn't need another language. Rust already won the language war — it just lost the adoption war.

Crust fixes adoption. Same syntax. Same compiler. Same binary. Different learning curve.

**Rust for the rest of us.**

---

## License

MIT

## Authors

The Crust Authors
