.PHONY: build cli tui test test-py lint differential differential-full benchmark clean

build:
	cargo build --release

cli:
	cargo build --release --bin rv-vectorize

tui:
	cargo build --release
	./target/release/rv-vectorize-tui

test:
	./scripts/test.sh

test-py:
	python3 ./scripts/test.py

lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

differential:
	./scripts/differential_test.py

differential-full:
	./scripts/differential_test.py --full

benchmark:
	./scripts/benchmark.py

clean:
	cargo clean
	rm -rf build
