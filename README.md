# crust

**Rust for the mass market.** A graduated-strictness toolchain that gives developers Python's ease of entry with Rust's performance ceiling — and a concrete migration path between them.

## The Problem

Rust is the most loved language seven years running. It's also the most abandoned.

The pattern repeats: developer hears Rust is fast and safe → tries Rust → hits the borrow checker on day one → gives up → goes back to Python/Go/TypeScript. The annual Rust survey consistently shows the same top pain points: steep learning curve, fighting the compiler, and slow productivity for newcomers.

Meanwhile, Python dominates AI/ML, data science, scripting, prototyping, and education — not because it's fast (it isn't), not because it's safe (it isn't), but because **you can sit down and just write code**. No type annotations required. No ownership model. No lifetime parameters. You think, you type, it runs.

The result: ~15M Python developers producing slow, untyped, GIL-constrained code. ~3M Rust developers producing fast, safe code that took 5x longer to write. And a mass of developers in between who'd *love* Rust's output but can't stomach Rust's input.

**This is a distribution problem, not a language problem.**

## The Insight

The borrow checker isn't the enemy — forcing it on day one is.

Every Rust concept exists for a reason: ownership prevents use-after-free, lifetimes prevent dangling references, the type system catches bugs at compile time. These are *good things*. But requiring a developer to understand all of them before they can print "Hello, world" is like requiring a driver's license before someone can sit in a car.

What if you could write Rust the way you write Python — and then *gradually* turn on the safety features as your code matures?

## What Crust Does

Crust is a Rust toolchain with a strictness dial:

| Level | Mode | What Changes |
|-------|------|-------------|
| **0** | Explore | Implicit `Clone` everywhere. No borrow checker. No lifetimes. `fn main()` optional. REPL. **This is Python-easy.** |
| **1** | Develop | Warnings on implicit clones. Suggestions for explicit ownership. Training wheels visible. |
| **2** | Harden | Borrow checker ON. Lifetime annotations required. Clippy-level lints. **This is real Rust.** |
| **3** | Ship | Full `rustc` semantics. Zero implicit allocations. `unsafe` audit. Deterministic builds. **This is production Rust.** |

The key: `crust migrate --to=3` emits a diff showing exactly what needs to change to go from your comfortable level-0 prototype to production-grade Rust. The compiler *teaches you* ownership by showing you where your implicit clones were hiding.

```rust
// hello.crust — Level 0
// No fn main(), no Result types, no borrow checker drama

let names = vec!["Alice", "Bob", "Charlie"];
let greeting = names;         // ← would FAIL in rustc (value moved)
println!("{:?}", names);       // ← Crust: silently cloned. Just works.
println!("{:?}", greeting);

let message = "Hello, " + "world" + "!";  // string concat just works
println!("{message}");
```

Then when you're ready:
```
$ crust migrate --to=2 hello.crust

  hello.crust:4  let greeting = names.clone();    // was: implicit clone
  hello.crust:5  println!("{:?}", &names);         // was: implicit borrow
  hello.crust:8  let message = format!("Hello, {}!", "world");  // was: + concat
```

## Architecture

```
                    ┌──────────────┐
   .rs / .crust ──▶│    Parser     │──▶ Crust AST
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  Desugaring  │  ← Implicit Clone/Arc insertion,
                    │   & Lowering │    main() wrapping, type inference
                    └──────┬───────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
        ┌──────────┐ ┌──────────┐ ┌──────────┐
        │Interpreter│ │ Cranelift│ │  rustc   │
        │ (tree-    │ │   JIT    │ │ codegen  │
        │  walk)    │ │          │ │          │
        └──────────┘ └──────────┘ └──────────┘
         Level 0      Level 0-2    Level 2-3
         REPL/scripts  fast builds  production
```

At the top of the dial, crust *emits standard Rust* and hands it to `rustc`. Your level-3 crust project IS a Rust project. No lock-in. No fork. The training wheels just come off.

## Market

**Total addressable**: Every developer who tried Rust and bounced. Every Python developer writing performance-sensitive code. Every team that wants Rust's safety guarantees but can't afford the onboarding time.

- Python: ~15.7M developers (Statista 2024), growing 25% YoY in AI/ML alone
- Rust: ~3.7M developers (JetBrains 2024), highest "want to learn" of any language
- The gap: 12M developers who want Rust's output but need Python's input

**Wedge**: AI/ML infrastructure. Python dominates the model layer, but the serving/inference layer is moving to Rust (Hugging Face candle, burn, ort). These teams need Rust performance yesterday and can't wait for their Python devs to grok lifetimes. Crust lets them ship level-0 code now and harden it to level-3 as the product matures.

**Expand**: Education (CS curricula adopting Rust), embedded/IoT (Arduino-class developers), systems programming (the next generation of infra engineers who grew up on Python).

## Why Now

1. **Rust is at an inflection point.** Adopted by Linux kernel, Android, Windows, AWS. The ecosystem is ready. The developer pipeline isn't.
2. **AI coding assistants need better targets.** LLMs generate mediocre Python. They could generate excellent level-0 Crust that *migrates* to safe Rust — a code-quality ratchet that doesn't exist today.
3. **The Python→Rust rewrite cycle is expensive and common.** Every ML company eventually rewrites their hot paths in Rust. Crust makes that a migration instead of a rewrite.

## Current Status

**v0.1.0** — Proof of concept. The toolchain skeleton works: `crust run`, `crust build` (real native binaries via rustc), interactive REPL, all flags parsed and acknowledged. The graduated-strictness architecture is designed ([DESIGN.md](DESIGN.md)) and the tree-walk interpreter is in development on the `v0.2-dev` branch.

```bash
$ cargo install --path .

$ crust                      # REPL
$ crust run program.crust    # Interpret
$ crust build -o myapp       # Native binary
```

## Roadmap

| Phase | Milestone | What Ships |
|-------|-----------|-----------|
| **1 — Interpreter** | Q3 2025 | Level-0 tree-walk interpreter. Variables, functions, structs, closures, `println!`. REPL. `.crust` file support. |
| **2 — Strictness** | Q4 2025 | Levels 1-2. Desugaring layer. `crust migrate`. Cranelift JIT for fast iteration. Warning-based teaching mode. |
| **3 — Production** | Q1 2026 | Level 3. `rustc` codegen emission. `crust build --release` = clean Rust → `cargo build --release`. Zero lock-in. |
| **4 — Ecosystem** | Q2 2026 | Crates.io integration. LSP/editor support. `crust init` project scaffolding. CI strictness gates. |

## Install

```bash
git clone https://github.com/jordanhubbard/crust.git
cd crust
cargo build --release
cp target/release/crust /usr/local/bin/
```

## Team

Built by systems engineers who've shipped operating systems, programming languages, and infrastructure at scale.

## License

MIT

---

See [DESIGN.md](DESIGN.md) for the full technical architecture.
