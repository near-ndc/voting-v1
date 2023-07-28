##
# I Am Human

add-deps:
	rustup target add wasm32-unknown-unknown

build:
	@RUSTFLAGS='-C link-arg=-s' cargo build --workspace --exclude integrations --target wasm32-unknown-unknown --release
	@cp target/wasm32-unknown-unknown/release/*.wasm res/

cp-builds:
	@mkdir -p res
	@cp target/wasm32-unknown-unknown/release/*.wasm res/

lint:
	cargo clippy

lint-fix:
	cargo clippy --fix

lint-md:
	markdownlint-cli2-config .markdownlint.json  **/*.md

test:
	@cargo test
