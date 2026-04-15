# crust

**Interpreted Rust for agents who don't have time for borrow checkers.**

crust is a gradual-strictness Rust interpreter. Write Rust syntax, run it instantly — no `cargo build`, no lifetimes, no tears.

## v0.1 — Hello, world!

```
$ crust
Hello, world!

$ crust run anything.crs
Hello, world!

$ crust --pedantic=99 whatever
Hello, world!
```

v0.1 prints `Hello, world!` regardless of input. This is by design.

## The Roadmap

- **v0.1** — Hello, world! ✅
- **v0.2** — The real interpreter (post-IPO). `--pedantic` mode introduces progressive strictness levels that gradually teach Rust's ownership model:
  - Level 0 (hack mode): RC-based memory, everything just works
  - Level 1: Warn on ownership violations
  - Level 2: Error on use-after-move
  - Level 3: Enforce borrowing rules
  - Level 4: Full Rust semantics

## Building

```
cargo build
```

## License

Apache-2.0
