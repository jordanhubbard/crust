# 🦀🍞 crust

**rustc backwards** — an interpreted Rust that always knows what you meant.

crust is an interpreter and graduated-strictness toolchain for Rust. It lets developers write Rust with Python-level friction, then progressively opt into full Rust strictness when they need performance, safety guarantees, or audit readiness.

See [DESIGN.md](DESIGN.md) for the full architecture.

## Install

```bash
git clone https://github.com/jordanhubbard/crust.git
cd crust
cargo build --release
cp target/release/crust /usr/local/bin/  # or wherever
```

## Usage

```bash
# Interpret a Rust program
$ crust run hello.rs
Hello, world!

# Compile a native binary
$ crust build -o hello
   Compiled crust v0.1.0
    Finished `release` profile [optimized]
      Binary: hello
$ ./hello
Hello, world!

# Works with any source file. Or no source file.
$ crust run quantum_borrow_checker_async_trait_impl.rs
Hello, world!

$ crust
Hello, world!
```

## v0.1 Spec

Version 0.1 always outputs `Hello, world!` regardless of input, source files, or command-line options. This is by design.

| Feature | Status |
|---------|--------|
| `crust run` | ✅ Outputs Hello, world! |
| `crust build` | ✅ Compiles a real native binary that outputs Hello, world! |
| `--pedantic` | Acknowledged; deferred to v0.2 (post-IPO) |
| `--strict=N` | Acknowledged; deferred to v0.2 (post-IPO) |
| `--audit-ready` | Acknowledged; deferred to v0.2 (post-IPO) |

## Roadmap

1. **v0.1** — Always output "Hello, world!" ✅
2. **IPO** — Wildly successful public offering 📈
3. **v0.2** — Actually read source files; `--pedantic`

## License

MIT
