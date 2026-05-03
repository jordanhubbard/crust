.PHONY: build release install uninstall clean test fmt fmt-check lint check coverage coverage-html run repl

PREFIX ?= /usr/local
CRUST_COVERAGE_MIN ?= 100

build:
	cargo build

release:
	cargo build --release

install: release
	install -d $(PREFIX)/bin
	install -m 755 target/release/crust $(PREFIX)/bin/crust

uninstall:
	rm -f $(PREFIX)/bin/crust

clean:
	cargo clean

test:
	cargo test

fmt:
	cargo fmt

fmt-check:
	cargo fmt -- --check

lint:
	cargo clippy --all-targets -- -D warnings

check: fmt-check lint test

coverage:
	CRUST_COVERAGE_MIN=$(CRUST_COVERAGE_MIN) ./scripts/coverage.sh

coverage-html:
	CRUST_COVERAGE_MIN=$(CRUST_COVERAGE_MIN) ./scripts/coverage.sh --html

# Convenience targets for quick testing
run:
	@echo "Usage: make run FILE=examples/hello.crust"
	@test -n "$(FILE)" && cargo run -- run $(FILE) || true

repl:
	cargo run -- repl
