# Testing And Coverage

Crust uses the standard Rust test harness plus `cargo-llvm-cov` for coverage.
All Prove-mode and verification-facing changes are expected to follow a strict
red, green, refactor loop:

1. Write or update the failing test that states the behavior.
2. Make the smallest implementation change that passes it.
3. Refactor only after the test is green.
4. Run the local quality gates before closing the bead.

The required local gate for normal code changes is:

```bash
make check
```

The strict coverage gate is:

```bash
make coverage
```

`make coverage` defaults to `CRUST_COVERAGE_MIN=100` and applies that threshold
to function, line, and region coverage. Lower thresholds are only for local
diagnostics, for example:

```bash
CRUST_COVERAGE_MIN=0 make coverage
```

The CI workflow uses a ratcheted baseline while the legacy suite is expanded to
the strict gate. That value must only move upward; the developer default remains
100 so verification-facing work does not normalize partial coverage. The current
CI minimum is **40** — `make coverage` measures regions 41.68%, lines 43.70%,
functions 61.89% as of 2026-05-04. The same threshold applies to all three
metrics, so the floor is whichever metric is lowest. Modules with the largest
remaining gaps: `eval.rs` (~33% regions), `stdlib.rs` (~23%), `repl.rs`
(interactive, 0%), `main.rs` (entry-point, 0%; integration-tested via
subprocess but llvm-cov doesn't capture subprocess profiles).

Coverage exemptions must be explicit. Use `CRUST_COVERAGE_IGNORE_REGEX` only for
generated code or code that cannot execute by construction, and document every
active exemption in this file.

Current active exemptions: none.
