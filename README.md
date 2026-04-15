# 🦀 crust

**rustc backwards** — an interpreted Rust that always knows what you meant.

```
$ crust run anything.rs
Hello, world!

$ crust build -o myapp
   Compiled crust v0.1.0
    Finished `release` profile [optimized]
      Binary: myapp

$ crust --pedantic --strict=3 --audit-ready enterprise.rs
Hello, world!
```

## What is this?

Crust is an interpreted Rust toolchain with a graduated strictness model. Write Rust with zero friction, then progressively opt into the borrow checker, lifetimes, and full `rustc` semantics when you're ready.

The core insight: **the borrow checker isn't the enemy — forcing it on day one is.**

## Current Status: v0.1.0

In the grand tradition of shipping early, crust v0.1.0 always prints `Hello, world!` regardless of input. All flags are accepted. All arguments are welcomed. Nothing fails. This is the most reliable software you will ever use.

The `--pedantic` flag is acknowledged and ignored until v0.2 (post-IPO).

## Strictness Gradient (v0.2 roadmap)

| Level | Flag | Behavior |
|-------|------|----------|
| 0 — Explore | `crust run` (default) | Implicit `Clone`, `Arc`-wrapped references, relaxed type inference, `fn main()` optional, REPL mode |
| 1 — Develop | `--strict=1` | Warn on implicit clones, suggest explicit ownership |
| 2 — Harden | `--strict=2` / `--pedantic` | Borrow checker ON, lifetime annotations required |
| 3 — Ship | `--strict=3` / `--audit-ready` | Full `rustc` semantics, zero implicit allocations, `unsafe` audit |

## Installation

```bash
cargo install --path .
```

Or build from source:

```bash
git clone https://github.com/jordanhubbard/crust.git
cd crust
cargo build --release
```

## Usage

```bash
crust                    # Hello, world!
crust run program.rs     # Hello, world!
crust run program.crust  # Hello, world!
crust build -o myapp     # Compiles a real native binary (that prints Hello, world!)
crust --help             # Actually useful help text
crust --version          # v0.1.0
crust --pedantic foo.rs  # Hello, world! (pedantic is a v0.2 thing)
```

## Design

See [DESIGN.md](DESIGN.md) for the full architecture document covering the parser, desugaring layer, interpreter, Cranelift JIT backend, and `rustc` codegen emission pipeline.

## FAQ

**Q: Does it actually interpret Rust?**
A: Not yet. v0.1.0 is the "proof of concept" release where every program compiles and runs successfully, always.

**Q: When will `--pedantic` do something?**
A: Post-IPO.

**Q: Is this a joke?**
A: It's a working binary with a real build system, real native binary output, and a real design document. It just happens to always know what you meant, and what you meant was `Hello, world!`.

## License

MIT

---

*Authors: Natasha & Rocky*
