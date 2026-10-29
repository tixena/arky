# Default: list recipes.
default:
    @just --list

# Lint Rust (clippy + fmt check).
lint: lint-rust

lint-rust:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo clippy -p arky --all-targets -- -D warnings
    cargo clippy -p arky --all-targets --features binary-format -- -D warnings
    cargo clippy -p arky-cli --all-targets --no-default-features -- -D warnings
    cargo fmt --all --check

build: build-rust

build-rust:
    cargo build

# Check the workspace on the minimum supported Rust version.
check-msrv:
    cargo +1.98 check --workspace --all-features

# Run the Rust test suite.
test: test-rust

test-rust:
    cargo test --workspace
    cargo test --workspace --all-features

# Run the store tests against servers in containers this recipe starts and stops.
test-stores:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo clippy -p arky-store -p arky-cli --all-targets --all-features --config 'build.rustflags=["--cfg", "store_tests"]' -- -D warnings
    name="arky-test-mongo-$$"
    # MongoDB's recommended open-files limit; the store tests keep more WiredTiger files open than Docker's default 1024.
    docker run -d --rm --ulimit nofile=64000:64000 --name "$name" -p 127.0.0.1::27017 mongo:latest >/dev/null
    trap 'docker stop "$name" >/dev/null' EXIT
    rustfs="arky-test-rustfs-$$"
    docker run -d --rm --name "$rustfs" -p 127.0.0.1::9000 \
      -e RUSTFS_ACCESS_KEY=arky-test -e RUSTFS_SECRET_KEY=arky-test-secret \
      rustfs/rustfs:1.0.0-beta.8 >/dev/null
    trap 'docker stop "$name" "$rustfs" >/dev/null' EXIT
    port=$(docker port "$name" 27017/tcp | head -1 | sed 's/.*://')
    rustfs_port=$(docker port "$rustfs" 9000/tcp | head -1 | sed 's/.*://')
    endpoint="http://127.0.0.1:$rustfs_port"
    until docker exec "$name" mongosh --quiet --eval 'db.runCommand({ ping: 1 }).ok' >/dev/null 2>&1; do sleep 1; done
    for _ in $(seq 1 50); do curl -s -o /dev/null "$endpoint/" && break; sleep 0.2; done
    curl -sf --aws-sigv4 "aws:amz:us-east-1:s3" --user arky-test:arky-test-secret -X PUT "$endpoint/arky-test"
    ARKY_TEST_MONGODB_URI="mongodb://127.0.0.1:$port" \
    ARKY_TEST_MINIO_ENDPOINT="$endpoint" ARKY_TEST_MINIO_BUCKET=arky-test \
    AWS_ACCESS_KEY_ID=arky-test AWS_SECRET_ACCESS_KEY=arky-test-secret \
      cargo test -p arky-store -p arky-cli --all-features --config 'build.rustflags=["--cfg", "store_tests"]'

# Full pipeline: lint everything, build everything, run tests.
all: lint test test-stores
