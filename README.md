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

Every one of those top 10 errors has a reasonable default that a beginner doesn't need to understand yet:

| Error | What Crust does at Level 0 |
|-------|---------------------------|
| E0277 — Missing trait impl | Auto-derive common traits (Debug, Clone, Display) |
| E0308 — Type mismatch | Implicit coercion where safe (i32 ↔ i64, &str ↔ String) |
| E0599 — Method not found | Suggest + auto-import, try common iterator adapters |
| E0382 — Use after move | Implicit clone |
| E0282 — Can't infer type | Widen inference, default to concrete types |
| E0106 — Missing lifetime | Elide aggressively, default to `'_` |

The code is still Rust. The binary is still Rust. The developer just doesn't get punched in the face on day one.

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

At Level 3, `crust build` and `rustc` produce identical output. Because the code was always Rust — it just had a patient teacher.

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

```bash
cargo install crust

crust run hello.crust           # interpret + run
crust build hello.crust -o app  # compile to native binary
crust build --emit-rs lib.crust # see the Rust that Crust generates
```

---

## Current Status

**v0.2.0** — Level 0 complete.

The Level 0 interpreter now covers essentially all of Rust's expression language:

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

See [DESIGN.md](DESIGN.md) for the full technical architecture and roadmap.

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
