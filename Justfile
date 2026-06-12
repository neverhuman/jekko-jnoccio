set shell := ["bash", "-euo", "pipefail", "-c"]

default: fast

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

security:
	bash ops/ci/security.sh

score:
	bash ops/ci/jankurai.sh

score-fast:
	bash ops/ci/jankurai.sh --fast

ci-doctor:
	bash scripts/ci-doctor.sh

ci-local:
	bash scripts/ci-local.sh
