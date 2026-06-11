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
