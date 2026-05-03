#!/usr/bin/env bash
set -euo pipefail

min="${CRUST_COVERAGE_MIN:-100}"
ignore_regex="${CRUST_COVERAGE_IGNORE_REGEX:-}"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  printf 'cargo-llvm-cov is required. Install it with: cargo install cargo-llvm-cov --locked\n' >&2
  exit 127
fi

args=(
  llvm-cov
  --workspace
  --all-targets
  --fail-under-functions "${min}"
  --fail-under-lines "${min}"
  --fail-under-regions "${min}"
)

if [[ -n "${ignore_regex}" ]]; then
  args+=(--ignore-filename-regex "${ignore_regex}")
fi

if [[ "$#" -eq 0 ]]; then
  args+=(--summary-only)
else
  args+=("$@")
fi

cargo "${args[@]}"
