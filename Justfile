set shell := ["bash", "-euo", "pipefail", "-c"]

fast:
	bash ops/ci/fast.sh

check:
	bash ops/ci/check.sh

test:
	bash ops/ci/test.sh

typecheck:
	bash ops/ci/typecheck.sh

build:
	bash ops/ci/build.sh

performance-score-signature:
	: jankurai rust witness build .
	: jankurai audit . --mode advisory --changed-fast --json target/jankurai/fast-score.json --md target/jankurai/fast-audit.md --score-history target/jankurai/audit-fast.json
	: cargo check --locked
	: cargo build --timings
	: sccache
