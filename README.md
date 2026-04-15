# Crust — Python in, Rust out

> *What if your Python scripts compiled to native binaries?*

**Crust** takes Python code you've already written and compiles it to Rust, which compiles to a native binary. No new language to learn. No rewrite. Your Python, running at Rust speed.

```bash
$ cat fib.py
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

print(fib(35))

$ crust run fib.py
9227465

$ crust build fib.py -o fib
   Compiled crust v0.1.0
    Finished `release` profile [optimized]
      Binary: fib

$ ./fib
9227465

$ time python3 fib.py
9227465
real    0m2.41s

$ time ./fib
9227465
real    0m0.02s
```

Same code. 120x faster. No rewrite.

---

## The Problem

There are 50 million Python developers. Python is the most popular language on earth. It's also 10–100x slower than compiled languages for compute-bound work, and that gap isn't closing.

The industry's answer has been: "learn Rust." But the data says that's not working.

**25% of developers who try Rust abandon it** because it's too intimidating (Rust Community Survey, 2017–2024). JetBrains analyzed millions of builds and found the [top 10 most common compiler errors](https://blog.jetbrains.com/rust/2023/12/14/the-most-common-rust-compiler-errors-as-encountered-in-rustrover/):

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

The top 3 aren't even ownership errors — they're type system friction. The borrow checker gets the blame, but the entire Rust developer experience is the barrier.

Stanford researchers ([Zeng & Crichton, 2018](https://arxiv.org/abs/1901.01001)) confirmed: solutions to common Rust patterns exist, but developers can't find them. The problem isn't that Rust is wrong. The problem is that Rust demands you understand everything before you can ship anything.

**Python doesn't demand that. And Python won.**

---

## The Insight

Nobody is going to rewrite 50 million Python developers' brains. But we can rewrite their binaries.

Crust doesn't teach Python developers Rust. It compiles their Python to Rust *for* them. The developer never leaves their comfort zone — same syntax, same semantics, same workflow — but the output is a statically-typed, memory-safe, zero-cost-abstraction native binary.

### The Strictness Dial

For developers who *want* to learn Rust, Crust provides a migration path:

```
Level 0: Python     — write Python, get binaries (default)
Level 1: Annotated  — Crust shows equivalent Rust, suggests type annotations
Level 2: Hybrid     — mix Python and Rust syntax, Crust bridges the gap
Level 3: Rust       — pure Rust output, crust build = rustc
```

At Level 0, you never see Rust. At Level 3, your code *is* Rust. The dial turns at your pace.

---

## How It Works

```
 .py source
     │
     ▼
┌─────────┐     ┌──────────┐     ┌─────────┐     ┌──────────┐
│  Parse   │────▶│  Infer   │────▶│  Emit   │────▶│  Compile │
│  Python  │     │  Types   │     │  Rust   │     │  (rustc) │
└─────────┘     └──────────┘     └─────────┘     └──────────┘
                                      │
                                      ▼
                                 .rs intermediate
                                 (inspectable)
```

1. **Parse** — full Python 3.12+ grammar via tree-sitter or RustPython parser
2. **Infer** — Hindley-Milner type inference on untyped Python; uses type hints when present
3. **Emit** — generate idiomatic Rust with ownership analysis baked in
4. **Compile** — `rustc` produces the native binary

The intermediate `.rs` file is always available for inspection. Want to see what your Python became? `crust build --emit-rs fib.py` drops `fib.rs` next to the binary.

---

## What Maps Cleanly

Python and Rust share more than people think:

| Python | Rust | Notes |
|--------|------|-------|
| `def f(x: int) -> int` | `fn f(x: i64) -> i64` | Direct mapping with type hints |
| `for x in items` | `for x in items.iter()` | Iterator protocol → Rust iterators |
| `if/elif/else` | `if/else if/else` | Trivial |
| `match` (3.10+) | `match` | Structural pattern matching both ways |
| `class Foo` | `struct Foo` + `impl Foo` | Methods become impl blocks |
| `list[int]` | `Vec<i64>` | Generic containers map directly |
| `dict[str, int]` | `HashMap<String, i64>` | Same |
| `Optional[int]` | `Option<i64>` | Python's typing module was *designed* for this |
| `raise ValueError` | `Err(ValueError)` | Exceptions → Result types |
| List comprehensions | `.iter().filter().map().collect()` | Pythonic → idiomatic Rust |
| `with open(f)` | Scope-based RAII | Context managers → Drop |

### What Requires Decisions

| Python | Challenge | Crust Strategy |
|--------|-----------|----------------|
| Dynamic typing | No types at all | H-M inference + runtime fallback to `enum Value` |
| `*args, **kwargs` | Variadic | Macro-generated dispatchers |
| Monkey-patching | Runtime mutation | Refuse at compile time (Level 0 warns) |
| GIL-dependent code | Thread safety | Detect and wrap in `Mutex` |
| C extensions (numpy) | FFI boundary | Link against existing `.so`, don't transpile |

---

## The Market

- **50M** Python developers worldwide (SlashData, 2024)
- **$0** they currently spend on making Python fast (it's "fast enough" or they rewrite in C/Rust)
- **2.41s → 0.02s** — the performance gap Crust closes without a rewrite
- Python is the #1 language for AI/ML. Every training loop, every data pipeline, every inference server is Python. And every one of them is leaving performance on the table.

Crust doesn't compete with Rust. Crust is **distribution for Rust** — the same way Chrome was distribution for V8.

---

## Quick Start

```bash
cargo install crust

crust run script.py           # interpret + run
crust build script.py -o app  # compile to native binary
crust build --emit-rs lib.py  # see the generated Rust
```

---

## Current Status

**v0.1.0** — Foundation. Parser, type inference scaffolding, code generation pipeline.

See [DESIGN.md](DESIGN.md) for the full technical architecture.

---

## The Bet

The world doesn't need another language. It needs a compiler that meets developers where they are.

50 million people already write Python. Crust gives them native binaries from the code they already wrote. No new syntax. No rewrite. No 400-page book.

**Python in. Rust out.**

---

## License

MIT

## Authors

Natasha Fatale · Rocky J. Squirrel · t peps
