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
| E0277 — Missing trait impl | Auto-derive `Clone`, `Debug`, `PartialEq` on user types; merge with author-supplied derives | Implemented |
| E0382 — Use after move | Implicit clone on identifier-shaped argument and field positions | Partial — see [bd](#known-gaps) `crust-ovw` |
| E0308 — Type mismatch | Safe numeric coercion in the interpreter | Partial — codegen does not always preserve widths (`crust-6yj`) |
| E0282 — Can't infer type | Turbofish round-trips, integer literal defaulting | Implemented |
| E0599 — Method not found | Auto-resolve common stdlib methods | Partial — no fuzzy match yet (`crust-k15`) |
| E0432 — Unresolved import | Auto-import std prelude paths | **Not implemented** (`crust-k15`) |
| E0425 — Unresolved name | Fuzzy match + suggest | **Not implemented** (`crust-k15`) |
| E0106 — Missing lifetime | Elide aggressively | **Not implemented** (`crust-1x4`, `crust-ovw`) |

The code is still Rust. The generated binary is still produced by `rustc`. The
developer doesn't get punched in the face on day one — but the strictness dial
is still under active development; see [Current Status](#current-status) and
the open beads issues for what's done vs in flight.

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

> **Current state:** Levels 0 and 4 are partly wired (Level 0 codegen + Level 4
> contract / SMT scaffolding). Levels 1, 2, 3 still mostly affect diagnostics
> rather than enforcement — tracked in `crust-o3a`. The "Level 3 ↔ rustc parity"
> claim is aspirational pending full ownership-relaxation analysis (`crust-ovw`)
> and a real compatibility contract (`crust-u5k`).

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
```

`crust` accepts a single `.crust` file with `std`-only imports today. Cargo
project / external-crate support is tracked in `crust-ti9`.

## Development

Run `make check` before closing code changes. See [docs/testing.md](docs/testing.md)
for the TDD and coverage policy.

---

## Current Status

**v0.2.0** — Level 0 interpreter is broadly working; codegen is partial.

The Level 0 **interpreter** (`crust run`) covers most of Rust's expression
language for self-contained programs. The Level 0 **code generator**
(`crust build`) compiles a useful subset to native binaries via `rustc` —
small examples round-trip cleanly, but anything that relies on real ownership
analysis (`.iter()` reference semantics, lifetime elision, refutable patterns
in `let`) does not yet emit valid Rust. See the open beads tracker for the
authoritative list.

Implemented in the interpreter:

- **All primitive types**: `i8`–`i64`, `u8`–`u64`, `f32`/`f64`, `bool`, `char`, `str`/`String`
- **Collections**: `Vec`, `HashMap`, `HashSet`, `BTreeMap`, `VecDeque` (all backed by Vec/HashMap at Level 0)
- **Traits**: definition, implementation, default methods, `dyn Trait`, `impl Trait`, operator overloading (`Add`, `Mul`, `Neg`, etc.)
- **Closures**: capturing, `move`, `FnMut`, returning closures, higher-order functions
- **Pattern matching**: destructuring, guards, or-patterns, slice patterns `[a, b, ..]`, range patterns, `@` bindings
- **Error handling**: `Result`/`Option` with `?` operator, combinators (`map`, `and_then`, `unwrap_or`, etc.)
- **Iterators**: `map`, `filter`, `fold`, `flat_map`, `zip`, `enumerate`, `chain`, `scan`, `partition`, `unzip`, custom `Iterator` impls
- **Control flow**: `for`, `while`, `loop`, labeled breaks, `while let`, `if let`, `let-else`
- **Generics**: generic functions and structs (type-erased at Level 0)
- **String formatting**: width, alignment, fill, precision, hex/bin/oct, named args
- **Associated constants** (`impl Type { const FOO: T = v; }`)
- **Array repeat syntax** (`[val; N]`)

### Known gaps

These are tracked in the beads issue database (run `bd ready` to see priorities):

- `crust-ovw` — Level 0 ownership-relaxation analysis is not implemented; `.iter()`/closure capture/move-after-clone scenarios fail to round-trip through `crust build`.
- `crust-1x4` — parser uses `skip_generics` / `skip_where` for many constructs; modules (`mod foo {}`), const generics, and where-clauses are not modelled.
- `crust-rvq` / `crust-ti9` — no module system, no Cargo.toml integration; `crust` accepts a single file with std-only imports.
- `crust-570` — `std::sync`, `std::thread`, `Arc`, `Mutex`, `mpsc::channel` are unimplemented.
- `crust-6yj` — primitive integer types collapse to `i64` in the interpreter; widths/signedness are not preserved.
- `crust-7e8` / `crust-v8b` — `--strict=4` SMT discharge is consistency-only without a body interpreter; emitted Coq/Lean files are uninterpreted-axiom skeletons.
- `crust-o3a` — Levels 1–3 are mostly diagnostics-shaped; the strictness dial does not yet engage rustc's borrow checker or clippy.

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

Natasha Fatale · Rocky J. Squirrel · t peps
