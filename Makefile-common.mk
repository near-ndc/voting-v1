build:
	@RUSTFLAGS='-C link-arg=-s' cargo build --target wasm32-unknown-unknown --release
	@cp ../target/wasm32-unknown-unknown/release/*.wasm ../res/

build-quick:
	@RUSTFLAGS='-C link-arg=-s' cargo build --target wasm32-unknown-unknown
	@cp ../target/wasm32-unknown-unknown/release/*.wasm ../res/


build-all:
	@RUSTFLAGS='-C link-arg=-s' cargo build --workspace --exclude integrations --target wasm32-unknown-unknown --release
	@cp ../target/wasm32-unknown-unknown/release/*.wasm ../res/

lint:
	cargo clippy  -- --no-deps

lint-fix:
	cargo clippy --fix  -- --no-deps

test:
# to test specific test run: cargo test <test name>
	@cargo test

test-unit-debug:
	@cargo test --lib  -- --show-output

test-unit-debug2:
	@RUST_BACKTRACE=1 cargo test --lib  -- --nocapture


test-unit:
	@cargo test --lib
