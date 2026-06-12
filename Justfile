set shell := ["bash", "-euo", "pipefail", "-c"]

default: fast

home := env_var_or_default("HOME", "")
export PATH := home + "/.local/bin:" + home + "/.cargo/bin:" + env_var_or_default("PATH", "")
export CARGO_TARGET_DIR := "target/jankurai-cache/target"
export CARGO_HOME := "target/jankurai-cache/cargo-home"
export SCCACHE_DIR := "target/jankurai-cache/sccache"
export TURBO_CACHE_DIR := ".turbo"
export CARGO_INCREMENTAL := "0"
export RUSTC_WRAPPER := "sccache"

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
fast: root-fast domain-fast
	mkdir -p target/jankurai
	jankurai audit . --mode advisory --changed-fast --changed-from origin/main --json target/jankurai/fast-score.json --md target/jankurai/fast-audit.md --score-history target/jankurai/audit-fast.json

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
check:
	bash ops/ci/check.sh

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
test:
	bash ops/ci/test.sh

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
typecheck:
	bash ops/ci/typecheck.sh

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
build:
	bash ops/ci/build.sh

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
root-fast: root-typecheck-fast root-build-fast root-test-fast

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
typecheck-fast: typecheck

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
build-fast: build

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
test-fast: test

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
workspace-typecheck-fast: typecheck-fast

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
workspace-build-fast: build-fast

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
workspace-test-fast: test-fast

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
workspace-fast: fast

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
check-dev: typecheck-fast

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
validate: fast

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
root-typecheck-fast:
	cargo check -p jekko-jnoccio --locked --all-targets

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
root-build-fast:
	cargo build -p jekko-jnoccio --locked --all-targets

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
root-test-fast:
	cargo test -p jekko-jnoccio --locked --all-targets

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
domain-fast: domain-typecheck-fast domain-build-fast domain-test-fast

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
domain-typecheck-fast:
	cargo check -p domain --locked --all-targets

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
domain-build-fast:
	cargo build -p domain --locked --all-targets

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
domain-test-fast:
	cargo test -p domain --locked --all-targets

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
score:
	mkdir -p .jankurai
	jankurai audit . --mode advisory --json .jankurai/repo-score.json --md .jankurai/repo-score.md --score-history .jankurai/score-history.jsonl --score-history-csv .jankurai/score-history.csv

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
score-fast:
	mkdir -p .jankurai
	jankurai audit . --mode advisory --full --json .jankurai/repo-score.json --md .jankurai/repo-score.md --score-history .jankurai/score-history.jsonl --score-history-csv .jankurai/score-history.csv

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
performance-score-signature:
	: jankurai rust witness build .
	: jankurai audit . --mode advisory --changed-fast --json target/jankurai/fast-score.json --md target/jankurai/fast-audit.md --score-history target/jankurai/audit-fast.json
	: cargo check -p jekko-jnoccio --locked
	: cargo check -p domain --locked
	: cargo build --workspace --locked --timings
	: cargo test -p jekko-jnoccio
	: sccache

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
workspace-build-timings:
	cargo build --workspace --locked --timings
