all: check build test

export RUSTFLAGS=-Dwarnings -Dclippy::all -Dclippy::pedantic

install:
	cargo install --path .

build:
	cargo build

build-test-wasms:
	cargo build --package 'test_*' --profile test-wasms --target wasm32-unknown-unknown

test: build-test-wasms
	cargo test --workspace

e2e-test:
	cargo test --test 'e2e*' -- --ignored

check:
	cargo clippy --all-targets --target aarch64-apple-darwin

watch:
	cargo watch --clear --watch-when-idle --shell '$(MAKE)'

fmt:
	cargo fmt --all

clean:
	cargo clean

publish:
	cargo workspaces publish --all --force '*' --from-git --yes
