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

No borrow checker. No lifetime annotations. Implicit `Clone` on every move. Auto-derive `Debug`, `Clone`, `Display` on user types. Implicit coercion between compatible types (`i32` ↔ `i64`, `&str` ↔ `String`). Type inference fills in everything it can.

**Goal:** A Python developer can write Rust syntax and have it run on the first try.

**What Crust does behind the scenes:**

| rustc error | Crust Level 0 behavior |
|-------------|----------------------|
| E0277 — Missing trait impl | Auto-derive common traits |
| E0308 — Type mismatch | Implicit safe coercion |
| E0599 — Method not found | Auto-import, suggest iterator adapters |
| E0382 — Use after move | Implicit clone |
| E0282 — Can't infer type | Widen inference, default to concrete types |
| E0106 — Missing lifetime | Aggressive elision, default `'_` |
| E0425 — Unresolved name | Fuzzy match + suggest |
| E0432 — Unresolved import | Auto-resolve from std/common crates |

### Level 1: Develop

Warnings on implicit clones. Type mismatch hints. Shadow detection. Lifetime suggestions. The compiler becomes a mentor — it tells you what it would complain about at Level 2, but lets you keep going.

### Level 2: Harden

Borrow checker active. Must annotate lifetimes. Explicit types required at function boundaries. Implicit clones become errors. This is "real Rust with slightly more helpful error messages."

### Level 3: Ship

Full `rustc` parity. `cargo clippy` clean. Zero-cost abstractions enforced. `crust build --strict=3` produces the exact same binary as `rustc`. The code IS Rust — rename `.crust` to `.rs` and ship it.

---

## Parser

Hand-written recursive descent. No dependency on `syn` or proc-macro infrastructure.

Targets the full Rust grammar with extensions:
- Strictness level directives: `#![crust(strict = 0)]`
- Per-function strictness: `#[crust(strict = 2)]` on individual functions
- Auto-derive hints: `#[crust(derive_all)]`

The parser produces a typed AST that feeds both the interpreter (`crust run`) and the code generator (`crust build`).

---

## Interpreter (crust run)

Tree-walk evaluator over the AST. Supports:

- Variables, functions, closures
- Control flow: if/else, match, for, while, loop, break, continue, return
- Structs, enums, impl blocks, methods
- Traits (basic dispatch)
- Generics (monomorphized at interpretation time)
- Standard library subset: Vec, HashMap, String, Option, Result, iterators

At Level 0, the interpreter handles ownership by implicit cloning — every value is reference-counted internally. This is semantically equivalent to Python's object model, which is the point.

---

## Code Generator (crust build)

Emits valid Rust source from the Crust AST, then invokes `rustc`.

At Level 0, the generated Rust includes:
- `#[derive(Clone, Debug)]` on all structs/enums
- Explicit `.clone()` calls where the original code would trigger E0382
- Type annotations inferred by the type checker
- `use` statements for auto-resolved imports

At Level 3, the generated Rust is identical to what a human would write.

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

## Roadmap

| Version | Milestone |
|---------|-----------|
| **0.1** | Foundation — `crust run` and `crust build` pipeline |
| **0.2** | Full interpreter: functions, structs, enums, traits, iterators, closures |
| **0.3** | Code generator: emit `.rs` from AST, `--emit-rs` flag |
| **0.4** | Strictness Levels 0-1 with progressive warnings |
| **0.5** | Levels 2-3, full borrow checker integration |
| **1.0** | Production — real-world Rust codebases, IDE integration, crate ecosystem |

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
