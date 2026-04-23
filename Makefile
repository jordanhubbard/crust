.PHONY: build release install uninstall clean test run repl

PREFIX ?= /usr/local

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

# Convenience targets for quick testing
run:
	@echo "Usage: make run FILE=examples/hello.crust"
	@test -n "$(FILE)" && cargo run -- run $(FILE) || true

repl:
	cargo run -- repl
