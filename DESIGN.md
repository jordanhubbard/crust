# Crust — An Interpreted Rust for Rapid Prototyping

## Mission

**Crust** is an interpreter for a practical subset of Rust syntax, designed for rapid prototyping and scripting. Write Rust-like code, run it instantly — no `cargo build`, no fighting the borrow checker on day one.

The key insight: Rust's *syntax* is excellent. Rust's *semantics* are excellent. But the compile-edit-run cycle and the borrow checker's learning curve are barriers to exploration. Crust lets you write Rust syntax with training wheels, then progressively tighten the screws until you're writing real Rust.

## Design Principles

1. **Valid Rust syntax** — Every crust program should be parseable as Rust (modulo a few convenience extensions). The goal is graduation: code written in crust should be trivially portable to `rustc`.

2. **Gradual strictness** — By default, crust runs in "hack mode" where memory is reference-counted, types can be inferred loosely, and lifetimes don't exist. The `--pedantic` flag (with levels 1-3) progressively enables Rust's stricter semantics.

3. **Instant feedback** — No compilation step. `crust run foo.rs` executes immediately. REPL mode with `crust repl`.

4. **Useful error messages** — When something fails, crust explains *why* Rust cares about this rule and *how* to fix it. It's a teaching tool, not just an executor.

## Strictness Levels

### Level 0: Hack Mode (default)
- All memory is reference-counted (Arc<T> under the hood)
- Type inference is generous (duck-typing for struct fields)
- No lifetime annotations needed
- `mut` is optional (everything is mutable by default)
- `unwrap()` on Option/Result is implicit (panics with good error messages)
- String literals are always `String`, not `&str`

### Level 1: `--pedantic=1` (Type Strict)
- Full type checking enforced
- No implicit unwrap — must handle Option/Result
- Generic types must be consistent
- Trait bounds checked

### Level 2: `--pedantic=2` (Ownership Aware)
- Move semantics enforced
- Borrowing rules checked (but no lifetime annotations required)
- `mut` required for mutation
- Single-owner semantics (no implicit RC)

### Level 3: `--pedantic=3` (Full Rust)
- Lifetime annotations required where Rust requires them
- Borrow checker fully active
- Essentially a Rust interpreter at this point

## Language Subset (v0.1)

### Supported
- `fn` declarations with typed parameters and return types
- `let` / `let mut` bindings
- Primitive types: `i32`, `i64`, `f64`, `bool`, `String`, `char`
- `Vec<T>`, `HashMap<K,V>`, `Option<T>`, `Result<T,E>`
- `struct` and `enum` (basic pattern matching)
- `if`/`else`, `while`, `for..in`, `loop`
- `match` expressions
- `impl` blocks (methods, associated functions)
- `trait` definitions and implementations (basic)
- Closures (basic)
- `println!`, `format!`, `vec!` macros
- String formatting with `{}`
- Tuple types and destructuring
- Range expressions (`0..n`, `0..=n`)

### Not in v0.1
- `async`/`await`
- Modules / `use` / `mod`
- Full macro system
- FFI
- Raw pointers
- `dyn` trait objects
- Complex lifetime annotations (level 3 stretch goal)

## Architecture

```
┌─────────────────────────────────────────────┐
│                   CLI                        │
│         crust run | crust repl               │
├─────────────────────────────────────────────┤
│                 Parser                       │
│    Source → Token Stream → AST               │
│    (Rust-compatible syntax)                  │
├─────────────────────────────────────────────┤
│              Type Checker                    │
│    Pedantic level determines strictness      │
│    Level 0: inference only                   │
│    Level 3: full Rust type system            │
├─────────────────────────────────────────────┤
│             Interpreter                      │
│    Tree-walking AST interpreter              │
│    RC-based memory (level 0-1)               │
│    Ownership tracking (level 2-3)            │
├─────────────────────────────────────────────┤
│            Standard Library                  │
│    Built-in: Vec, HashMap, String, IO        │
│    println!, format!, vec! macros            │
└─────────────────────────────────────────────┘
```

### Implementation Language

**Rust.** Yes, we're writing a Rust interpreter in Rust. This is the way.

- Fast enough for interactive use
- Can leverage `syn` crate for parsing (real Rust parser!)
- Can eventually `include!` or FFI into real Rust crates
- Dog-fooding: any crust contributor already knows the target language

### Key Crates
- `syn` — Rust parser (gives us a real Rust AST for free)
- `quote` — AST construction utilities
- `proc-macro2` — Token manipulation
- `rustyline` — REPL readline support
- `clap` — CLI argument parsing
- `miette` or `ariadne` — Pretty error reporting

### Why `syn`?

Using `syn` means we parse *actual* Rust syntax. No custom grammar, no "almost Rust" — if `syn` can parse it, it's valid Rust. This guarantees our principle that crust code is portable to `rustc`.

The interpreter walks the `syn::File` AST directly. No intermediate representation needed for v0.1.

## Example

```rust
// fibonacci.rs — runs in crust without any changes needed
fn fib(n: i32) -> i32 {
    match n {
        0 => 0,
        1 => 1,
        _ => fib(n - 1) + fib(n - 2),
    }
}

fn main() {
    for i in 0..10 {
        println!("fib({}) = {}", i, fib(i));
    }
}
```

```bash
$ crust run fibonacci.rs
fib(0) = 0
fib(1) = 1
fib(2) = 1
...
fib(9) = 34

$ crust run --pedantic=2 fibonacci.rs
# Same output — this code is already ownership-clean!

$ crust repl
crust> let x = vec![1, 2, 3];
crust> x.iter().map(|n| n * 2).collect::<Vec<_>>()
[2, 4, 6]
```

## Roadmap

### v0.1 — "It Lives"
- [ ] Parse Rust source via `syn`
- [ ] Interpret: functions, let bindings, if/else, loops, match
- [ ] Primitives: i32, f64, bool, String
- [ ] Vec, basic iterators
- [ ] println! macro
- [ ] REPL mode
- [ ] CLI: `crust run <file>` and `crust repl`

### v0.2 — "Getting Useful"
- [ ] Structs, enums, impl blocks
- [ ] Traits (basic)
- [ ] HashMap
- [ ] Option/Result with pattern matching
- [ ] Closures
- [ ] --pedantic=1 (type strictness)

### v0.3 — "The Reckoning"
- [ ] --pedantic=2 (ownership tracking)
- [ ] Move semantics simulation
- [ ] Borrow checker (basic)
- [ ] Better error messages with suggestions

### v0.4 — "Almost Rust"
- [ ] --pedantic=3 (full Rust semantics)
- [ ] Module system basics
- [ ] Cargo.toml awareness (read dependencies)
- [ ] Integration: `crust graduate` → emit clean Rust + Cargo.toml
