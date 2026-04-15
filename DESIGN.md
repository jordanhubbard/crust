# Crust — Design Document

> "rustc backwards" — an interpreted Rust that starts easy and gets strict on demand.

## 1. Vision

Crust is an interpreter and graduated-strictness toolchain for Rust. It lets developers write Rust with Python-level friction, then progressively opt into full Rust strictness when they need performance, safety guarantees, or audit readiness.

The core insight: **the borrow checker isn't the enemy — forcing it on day one is.** Crust defers ownership semantics until the developer is ready, using implicit cloning and reference counting as training wheels that can be removed.

## 2. Strictness Gradient

| Level | Flag | Behavior |
|-------|------|----------|
| 0 — Explore | `crust run` (default) | Implicit `Clone`, `Arc`-wrapped references, relaxed type inference, `fn main()` optional, REPL mode available |
| 1 — Develop | `--strict=1` | Warn on implicit clones, suggest explicit ownership, enforce basic type annotations |
| 2 — Harden | `--strict=2` / `--pedantic` | Borrow checker ON, lifetime annotations required where ambiguous, clippy-level lints |
| 3 — Ship | `--strict=3` / `--audit-ready` | Full `rustc` semantics, zero implicit allocations, `unsafe` audit, deterministic builds |

The migration path is: write at level 0, `crust migrate --to=3` emits a diff showing exactly what needs to change.

## 3. Architecture

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

### 3.1 Parser

- Parse standard Rust syntax via `syn` (the Rust `syn` crate, used as a library)
- Extended syntax for Crust sugar: bare expressions at top level, `?` without Result wrapping, optional semicolons
- `.crust` file extension for files using extended syntax; `.rs` files parsed as standard Rust

### 3.2 Desugaring Layer (the magic)

This is where strictness levels are applied:

- **Level 0**: Every owned value is implicitly `Clone`-able. References become `Arc<T>` under the hood. `let x = expensive_thing()` followed by `let y = x` just clones. No moves, no borrowing errors.
- **Level 1**: Same transforms but with warnings: "implicit clone of `x` here — consider `x.clone()` or `&x`"
- **Level 2**: Transforms removed. Real borrow checker semantics. Lifetime elision rules apply normally.
- **Level 3**: No desugaring at all — pure `rustc` passthrough.

### 3.3 Backends

**Tree-walk interpreter** (MVP, level 0 only):
- Direct AST interpretation, no compilation step
- Good for REPL, scripting, rapid prototyping
- Performance isn't the point — developer speed is

**Cranelift JIT** (phase 2):
- Compile Crust AST → Cranelift IR → native code in memory
- Sub-second compile times, near-native performance
- Supports levels 0–2

**rustc codegen** (phase 3):
- Emit desugared Rust source → feed to `rustc`
- For level 2–3, this IS the production path
- `crust build --release` = emit clean Rust + `cargo build --release`

## 4. Module Breakdown

```
crust/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Public API
│   ├── parser/
│   │   ├── mod.rs           # Parser orchestration
│   │   ├── rust_compat.rs   # Standard Rust parsing (via syn)
│   │   └── crust_sugar.rs   # Extended .crust syntax
│   ├── ast/
│   │   ├── mod.rs           # Crust AST types
│   │   └── visitor.rs       # AST walker trait
│   ├── desugar/
│   │   ├── mod.rs           # Desugaring pipeline
│   │   ├── clone_insert.rs  # Implicit Clone insertion
│   │   ├── arc_wrap.rs      # Reference → Arc conversion
│   │   ├── type_infer.rs    # Relaxed type inference
│   │   └── main_wrap.rs     # Bare expression → fn main()
│   ├── strictness/
│   │   ├── mod.rs           # Strictness level config
│   │   ├── diagnostics.rs   # Warnings/hints per level
│   │   └── migrate.rs       # Auto-migration between levels
│   ├── interpret/
│   │   ├── mod.rs           # Tree-walk interpreter
│   │   ├── env.rs           # Runtime environment/scoping
│   │   ├── value.rs         # Runtime value types
│   │   └── builtins.rs      # Built-in functions (println!, etc.)
│   ├── jit/                 # (Phase 2) Cranelift backend
│   │   ├── mod.rs
│   │   └── codegen.rs
│   ├── emit/                # (Phase 3) Rust source emission
│   │   ├── mod.rs
│   │   └── rustfmt.rs       # Clean output formatting
│   └── repl/
│       ├── mod.rs           # REPL loop
│       └── completion.rs    # Tab completion
├── tests/
│   ├── level0/             # Explore mode tests
│   ├── level1/             # Develop mode tests
│   ├── level2/             # Harden mode tests
│   ├── migration/          # Migration tests
│   └── compat/             # Standard Rust compatibility
└── examples/
    ├── hello.crust         # Bare expression demo
    ├── web_server.crust    # Async with training wheels
    └── data_pipeline.crust # Clone-heavy code that "just works"
```

## 5. MVP Scope (Phase 1)

**Goal**: `crust run hello.crust` works. REPL works. Level 0 only.

What's in:
- Parser for a subset of Rust (let bindings, functions, structs, enums, basic pattern matching, closures)
- Implicit Clone semantics (no borrow checker)
- Tree-walk interpreter with runtime values
- `println!` and basic formatting macros
- REPL mode (`crust` with no args)
- `.crust` file support (bare expressions, optional main)

What's deferred:
- Cranelift JIT (phase 2)
- Levels 1–3 (phase 2)
- `crust migrate` (phase 2)
- Async/await (phase 2)
- Full standard library (progressive)
- rustc codegen (phase 3)

## 6. Example: What Level 0 Looks Like

```rust
// hello.crust — no fn main(), no Result types, no borrow drama

let names = vec!["Alice", "Bob", "Charlie"];

// This would fail in Rust: "value moved here... value used after move"
// In Crust level 0: silently clones. It just works.
let greeting = names;
println!("Original: {:?}", names);  // ← would be a compile error in Rust
println!("Copy: {:?}", greeting);

// Structs work without derive macros
struct Point { x: f64, y: f64 }

let p = Point { x: 1.0, y: 2.0 };
let q = p;  // implicit clone
println!("p = ({}, {}), q = ({}, {})", p.x, p.y, q.x, q.y);

// String handling without &str vs String confusion
let name = "world";
let message = "Hello, " + name + "!";  // string concat just works
println!("{message}");
```

## 7. Implementation Language

Crust itself is written in Rust. We eat our own dog food at the toolchain level — the interpreter is a Rust binary. This means:
- We can leverage `syn` for parsing
- Cranelift integration is natural
- The `crust build --audit-ready` path literally emits Rust

## 8. Design Decisions

**`.crust` files are a superset of Rust.** Any valid `.rs` file is valid `.crust`. The extensions are additive: bare top-level expressions, optional `fn main()`, `+` string concat, relaxed semicolons. This means level-3 crust code is byte-identical to Rust. No fork, no dialect, no lock-in.

**Crate imports at level 0 use a bundled mini-stdlib.** Level 0 ships with `crust_std` providing ergonomic wrappers around common crates (HTTP, JSON, file I/O, async). `use crust::http` at level 0 becomes `use reqwest` at level 3. The migration tool handles the rewrite.

**Trait resolution uses dynamic dispatch at level 0, monomorphization at level 2+.** Level 0 prioritizes "it just works" over performance. The desugaring layer inserts `dyn Trait` where needed. Level 2 switches to static dispatch. The migration diff shows you every place this changes.

**Error messages always explain what rustc *would* say.** Even at level 0, when crust silently clones, the REPL can show `(hint: rustc would reject this — names was moved on line 4. crust cloned it for you.)` This turns the interpreter into a teaching tool.

**WASM target is phase 4.** A browser playground is high-value for adoption but not on the critical path. The interpreter's tree-walk architecture is WASM-friendly by design.

## 9. The Competitive Landscape

| Tool | What It Does | Why It's Not Enough |
|------|-------------|-------------------|
| Rust Playground | Browser REPL | Full rustc semantics — still hits the wall |
| Mojo | Python superset → fast | Different language. Proprietary. Not Rust. |
| Zig | Simple systems language | No Rust ecosystem. No graduated strictness. |
| PyO3 / rust-cpython | Rust ↔ Python FFI | Glue code, not migration. Two languages forever. |
| Rust `clippy` | Lint suggestions | Post-hoc. Doesn't change the entry barrier. |

Crust is the only tool that makes Rust code progressively stricter while maintaining a single codebase that converges to standard Rust.

## 10. Key Metrics

- **Time to first program**: Target <30 seconds for a developer with Python experience (level 0)
- **Migration coverage**: `crust migrate --to=3` should handle >90% of transforms automatically
- **Rustc compatibility**: Level-3 output must pass `cargo clippy` and `cargo test` unmodified
- **Performance at level 0**: Within 10x of native Rust (acceptable for prototyping; Cranelift JIT closes the gap)

---

*Authors: Natasha, Rocky, t peps*
*Status: Architecture finalized — implementation on v0.2-dev branch*
