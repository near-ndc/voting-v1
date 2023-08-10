res:
	mkdir -p res

build: res
	@RUSTFLAGS='-C link-arg=-s' cargo build --target wasm32-unknown-unknown --release
	@cp ../target/wasm32-unknown-unknown/release/*.wasm ../res/

build-debug: res
	@RUSTFLAGS='-C link-arg=-s' cargo build --target wasm32-unknown-unknown
	@cp ../target/wasm32-unknown-unknown/debug/*.wasm ../res/

build-abi: res
	@cargo near abi
	@cp ../target/near/*/*_abi.json ../res


build-all: res
	@RUSTFLAGS='-C link-arg=-s' cargo build --workspace --exclude integrations --target wasm32-unknown-unknown --release
	@cp ../target/wasm32-unknown-unknown/release/*.wasm ../res/

lint:
	cargo clippy  -- --no-deps

lint-fix:
	cargo clippy --fix  -- --no-deps

test:
# to test specific test run: cargo test <test name>
	@[ -f "../res/registry.wasm" ] || (echo "res/registry.wasm is required to run integration tests. Link it to the registry contract from the i-am-human repository" && exit 1)
	@cargo test

test-unit-debug:
	@cargo test --lib  -- --show-output

test-unit-debug2:
	@RUST_BACKTRACE=1 cargo test --lib  -- --nocapture


test-unit:
	@cargo test --lib
