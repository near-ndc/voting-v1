##
# I Am Human

res:
	mkdir -p res

add-deps:
	rustup target add wasm32-unknown-unknown

build: res
	@RUSTFLAGS='-C link-arg=-s' cargo build --workspace --exclude integrations --target wasm32-unknown-unknown --release
	@cp target/wasm32-unknown-unknown/release/*.wasm res/

lint:
	cargo clippy

lint-fix:
	cargo clippy --fix

lint-md:
	markdownlint-cli2-config .markdownlint.json  **/*.md

test:
	@[ -f "res/registry.wasm" ] || (echo "res/registry.wasm is required to run integration tests. Link it to the registry contract from the i-am-human repository" && exit 1)
	@cargo test

test-unit:
	@cargo test --lib
