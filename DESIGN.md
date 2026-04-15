# Crust — Technical Design

## One-Liner

Python in, Rust out. Accept `.py` files, emit `.rs`, compile to native binaries via `rustc`.

---

## Architecture

```
                         crust run / crust build
                                  │
                                  ▼
┌──────────────────────────────────────────────────────────┐
│                      FRONTEND                            │
│                                                          │
│  .py source ──▶ Python Parser ──▶ Crust IR (typed AST)  │
│                 (tree-sitter /                            │
│                  RustPython)                              │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│                   TYPE INFERENCE                          │
│                                                          │
│  Hindley-Milner unification over Crust IR                │
│  + type hint extraction (PEP 484/526/544)                │
│  + fallback: dynamic dispatch via enum Value             │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│                   RUST CODEGEN                            │
│                                                          │
│  Crust IR ──▶ Rust AST ──▶ .rs source text               │
│                                                          │
│  Ownership analysis:                                     │
│    - Single-use values → move                            │
│    - Multi-use values → clone (Level 0) / borrow (L2+)  │
│    - Escape analysis for heap vs stack                   │
│                                                          │
│  --emit-rs flag: write .rs alongside binary              │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│                    BACKEND                                │
│                                                          │
│  crust run:   interpret via Crust IR directly            │
│  crust build: .rs ──▶ rustc -C opt-level=2 ──▶ binary   │
└──────────────────────────────────────────────────────────┘
```

---

## Core Translation Rules

### Types

| Python | Crust IR | Rust |
|--------|----------|------|
| `int` | `Int` | `i64` |
| `float` | `Float` | `f64` |
| `str` | `Str` | `String` |
| `bool` | `Bool` | `bool` |
| `None` | `Unit` | `()` |
| `list[T]` | `List(T)` | `Vec<T>` |
| `dict[K, V]` | `Map(K, V)` | `HashMap<K, V>` |
| `set[T]` | `Set(T)` | `HashSet<T>` |
| `tuple[A, B]` | `Tuple(A, B)` | `(A, B)` |
| `Optional[T]` | `Option(T)` | `Option<T>` |
| No annotation | `Inferred` | H-M unification result |

### Functions

```python
# Python
def add(a: int, b: int) -> int:
    return a + b
```

```rust
// Emitted Rust
fn add(a: i64, b: i64) -> i64 {
    a + b
}
```

Untyped functions use Hindley-Milner inference. If inference fails (truly dynamic code), the function operates on `enum Value` with runtime dispatch.

### Classes → Structs + Impl

```python
class Point:
    def __init__(self, x: float, y: float):
        self.x = x
        self.y = y

    def distance(self, other: 'Point') -> float:
        return ((self.x - other.x)**2 + (self.y - other.y)**2)**0.5
```

```rust
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn distance(&self, other: &Point) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}
```

### Error Handling

```python
# Python exceptions
try:
    value = int(input())
except ValueError:
    value = 0
```

```rust
// Emitted Rust
let value = match input().parse::<i64>() {
    Ok(v) => v,
    Err(_) => 0,
};
```

Crust maps `try/except` to `match` on `Result`. Unhandled exceptions become `.unwrap()` at Level 0 and `?` propagation at Level 2+.

### Iterators & Comprehensions

```python
squares = [x**2 for x in range(10) if x % 2 == 0]
```

```rust
let squares: Vec<i64> = (0..10)
    .filter(|x| x % 2 == 0)
    .map(|x| x.pow(2))
    .collect();
```

List comprehensions → iterator chains. Generator expressions → lazy iterators. This is where Python developers accidentally learn Rust's best feature.

---

## Ownership Strategy

The hardest problem: Python has a GC. Rust doesn't.

### Level 0 (default): Clone Everything

At Level 0, every value that's used more than once gets cloned. This is "wrong" from a Rust purist perspective but *correct* from a Python semantics perspective — Python objects are reference-counted and effectively cloned on use.

The generated Rust is correct, safe, and runs. It's just not zero-cost. And it's still 50-100x faster than CPython.

### Level 1: Warn on Clones

Crust emits the binary but also prints:
```
hint: `data` is cloned 3 times — consider passing by reference
hint: `process(data)` takes ownership — last use, no clone needed
```

### Level 2: Borrow by Default

Crust performs escape analysis and generates `&` / `&mut` references where possible. Clones only where necessary. Lifetime annotations inferred.

### Level 3: Full Rust

No implicit clones. The generated Rust would pass `cargo clippy` with zero warnings. If the Python code can't be expressed without clones, Crust errors with a suggestion.

---

## Python Stdlib Mapping

Not everything translates. Crust maintains a stdlib compatibility layer:

### Tier 1 — Direct mapping (ships in v0.2)
- `print()` → `println!()`
- `len()` → `.len()`
- `range()` → `Range` / iterator
- `str.split/join/strip/replace` → `String` methods
- `list.append/pop/sort` → `Vec` methods
- `dict` operations → `HashMap` methods
- `math.*` → `f64` methods / `std::f64::consts`
- `os.path` → `std::path::Path`
- `json` → `serde_json`
- `re` → `regex` crate
- `sys.argv` → `std::env::args()`

### Tier 2 — Crate-backed (v0.3)
- `requests` → `reqwest`
- `datetime` → `chrono`
- `collections` → `std::collections`
- `itertools` → `itertools` crate
- `typing` → native Rust types

### Tier 3 — Runtime shim (v0.4)
- `asyncio` → `tokio`
- `threading` → `std::thread` + `rayon`
- `subprocess` → `std::process::Command`
- `sqlite3` → `rusqlite`

### Escape Hatch — FFI
For anything that can't translate (numpy, pandas, torch), Crust generates FFI bindings against the existing Python C extensions via PyO3. Your hot loop is native Rust; your numpy call goes through FFI. Still faster than pure CPython.

---

## Parser Strategy

Two options evaluated:

### Option A: tree-sitter-python (chosen for v0.2)
- Mature, battle-tested, incremental
- Concrete syntax tree preserves all source info
- Rust bindings via `tree-sitter` crate
- Same parser VS Code / Neovim use

### Option B: RustPython parser
- Full Python 3.12 grammar
- Produces Python AST directly
- More complete but heavier dependency

v0.2 uses tree-sitter for speed and incrementality. RustPython parser is the fallback for edge cases.

---

## What We're Not Building

- **A new language.** Crust accepts standard Python. Period.
- **A Python runtime.** We don't ship a GC, GIL, or bytecode VM.
- **A competitor to Mojo/Codon.** Those are new languages with Python syntax. Crust compiles *actual* Python.
- **A replacement for Rust.** Crust is distribution for Rust. When developers are ready, they graduate to `rustc` directly.

---

## Roadmap

| Version | Milestone |
|---------|-----------|
| **0.1** | Foundation — `crust run` and `crust build` pipeline, proof of concept |
| **0.2** | Python parser (tree-sitter), type inference, basic codegen for functions/classes/iterators |
| **0.3** | Stdlib Tier 1+2 mapping, `--emit-rs`, strictness Levels 0-1 |
| **0.4** | Ownership analysis, Levels 2-3, escape analysis, lifetime inference |
| **0.5** | PyO3 FFI escape hatch, async support via tokio |
| **1.0** | Production-grade — handles real-world Python codebases, IDE integration |

---

## Prior Art & Differentiation

| Project | Approach | Crust Difference |
|---------|----------|-----------------|
| **Mojo** | New language, Python-like syntax | Crust accepts *actual* Python |
| **Codon** | Python subset → LLVM | Crust targets Rust (inspectable, editable) |
| **Cython** | Python + C types → C extension | Crust produces standalone binaries |
| **Nuitka** | Python → C → binary | Bundles CPython runtime; Crust doesn't |
| **PyO3** | Rust ↔ Python FFI | Crust generates the Rust; PyO3 is our escape hatch |
| **RPython** | Python subset for PyPy | Language-level VM toolkit, not a user-facing compiler |

The key differentiator: **Crust emits readable Rust.** The output isn't object code or intermediate bytecode — it's `.rs` files a human can read, edit, and maintain. Crust is a migration tool, not a black box.
