# jankurai Repo Score

- Standard: `jankurai`
- Auditor: `1.6.1`
- Schema: `1.9.0`
- Paper edition: `2026.05-ed8`
- Target stack ID: `rust-ts-vite-react-postgres-bounded-python`
- Target stack: `Rust core + TypeScript/React/Vite + PostgreSQL + generated contracts + exception-only Python AI/data service`
- Repo: `.`
- Run ID: `1781228575`
- Started at: `1781228575`
- Elapsed: `4359` ms
- Scope: `full`
- Raw score: `77`
- Final score: `77`
- Decision: `advisory`
- Minimum score: `85`
- Caps applied: `none`

## Hard Rule Caps

| Rule | Max Score | Applied |
| --- | ---: | --- |
| `no-root-agent-instructions` | 75 | no |
| `no-one-command-setup-or-validation` | 70 | no |
| `no-deterministic-fast-lane` | 65 | no |
| `no-security-lane-on-high-risk-repo` | 60 | no |
| `generated-contracts-or-public-api-drift-untested` | 80 | no |
| `python-direct-product-truth-or-db-ownership` | 72 | no |
| `no-secret-or-dependency-scanning-in-ci` | 78 | no |
| `no-jankurai-audit-lane-in-ci` | 82 | no |
| `jankurai-required-tool-ci-evidence-gap` | 88 | no |
| `non-optimal-product-language-found` | 74 | no |
| `too-much-python-in-product-surface` | 72 | no |
| `boundary-reclassification-evidence-gap` | 72 | no |
| `vibe-placeholders-in-product-code` | 68 | no |
| `fallback-soup-in-product-code` | 70 | no |
| `future-hostile-dead-language-in-product-code` | 64 | no |
| `severe-duplication-in-product-code` | 70 | no |
| `generated-zone-mutation-risk` | 76 | no |
| `direct-db-access-from-wrong-layer` | 66 | no |
| `missing-web-e2e-lane` | 82 | no |
| `missing-rendered-ux-qa-lane` | 84 | no |
| `prompt-injection-risk` | 78 | no |
| `overbroad-agent-agency` | 65 | no |
| `secret-like-content-detected` | 60 | no |
| `false-green-test-risk` | 76 | no |
| `destructive-migration-risk` | 70 | no |
| `authz-or-data-isolation-gap` | 78 | no |
| `input-boundary-gap` | 78 | no |
| `agent-tool-supply-chain-gap` | 78 | no |
| `release-readiness-gap` | 80 | no |
| `missing-rust-property-or-integration-tests` | 82 | no |
| `no-agent-friendly-exception-pattern` | 76 | no |
| `missing-agent-readable-docs` | 80 | no |
| `streaming-runtime-drift` | 78 | no |
| `rust-bad-behavior` | 72 | no |
| `sql-bad-behavior` | 72 | no |
| `typescript-bad-behavior` | 72 | no |
| `docker-bad-behavior` | 72 | no |
| `python-bad-behavior` | 72 | no |
| `ci-bad-behavior` | 70 | no |
| `git-bad-behavior` | 70 | no |
| `gittools-bad-behavior` | 70 | no |
| `release-bad-behavior` | 70 | no |
| `web-security-bad-behavior` | 68 | no |
| `repo-rot-bad-behavior` | 88 | no |
| `comment-hygiene-dangerous-residue` | 72 | no |
| `ci-local-parity` | 70 | no |

## Copy-Code Redundancy

- Status: `review` hard=`0` warning=`16` files=`78`
- Policy: min-lines=`10` min-tokens=`100` max-findings=`50` include-tests=`false` strict=`false`
- Duplicate volume: lines=`30` tokens=`83` bytes=`859`

- Notes:
  - hard classes are limited to exact active-source file matches and substantial exact same-name units
  - warning classes include same-body different-name units and token/block duplication
  - tests, fixtures, stories, config, Docker, and migrations are omitted unless --include-tests is set

| Kind | Severity | Language | Lines | Tokens | Instances | Reason |
| --- | --- | --- | ---: | ---: | --- | --- |
| `ExactUnitDifferentName` | `Warning` | `rust` | 8 | 10 | `jnoccio-fusion/src/bin/provider_probe.rs:209-217, jnoccio-fusion/src/failure_log.rs:225-233` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 8 | `jnoccio-fusion/src/routing.rs:708-709, jnoccio-fusion/src/routing.rs:734-735, jnoccio-fusion/src/routing.rs:752-753, jnoccio-fusion/src/routing.rs:816-817, jnoccio-fusion/src/routing.rs:837-838` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 4 | 12 | `jnoccio-fusion/src/router.rs:519-523, jnoccio-fusion/src/router.rs:538-542` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 8 | `jnoccio-fusion/src/router.rs:696-700, jnoccio-fusion/src/rpc_shim.rs:557-561` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 1 | `crates/zyal-core/src/forbidden.rs:107-108, crates/zyal-core/src/forbidden.rs:115-116, crates/zyal-core/src/forbidden.rs:137-138, jnoccio-fusion/src/config.rs:1075-1076` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 6 | `jnoccio-fusion/src/mcp.rs:1090-1092, jnoccio-fusion/src/search.rs:187-189` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 7 | `jnoccio-fusion/src/config.rs:614-615, jnoccio-fusion/src/config.rs:725-726` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 6 | `jnoccio-fusion/src/fusion.rs:347-348, jnoccio-fusion/src/fusion.rs:2130-2131` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 6 | `jnoccio-fusion/src/fusion.rs:2874-2875, jnoccio-fusion/src/fusion.rs:2886-2887` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `jnoccio-fusion/src/routing.rs:698-699, jnoccio-fusion/src/routing.rs:719-720` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `jnoccio-fusion/src/routing.rs:558-559, jnoccio-fusion/src/routing.rs:568-569` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 3 | `jnoccio-fusion/src/rpc_shim.rs:59-60, jnoccio-fusion/src/rpc_shim.rs:65-66` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 2 | `jnoccio-fusion/src/routing.rs:766-767, jnoccio-fusion/src/routing.rs:793-794` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 2 | `jnoccio-fusion/src/state/state_util.rs:125-126, jnoccio-fusion/src/state/state_util.rs:161-162` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 1 | `jnoccio-fusion/src/config.rs:366-367, jnoccio-fusion/src/fusion.rs:3084-3085` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 1 | 1 | `jnoccio-fusion/src/router.rs:707-708, jnoccio-fusion/src/rpc_shim.rs:565-566` | `same-name semantic unit copied across multiple files` |

## Dimensions

| Dimension | Weight | Score | Weighted | Evidence |
| --- | ---: | ---: | ---: | --- |
| Ownership and navigation surface | 13 | 86 | 11.18 | root `AGENTS.md` present; owner map present |
| Contract and boundary integrity | 13 | 100 | 13.00 | contract surface found; generated contract artifacts found |
| Proof lanes and test routing | 12 | 96 | 11.52 | one-command setup/validation lane found; deterministic fast lane found |
| Security and supply-chain posture | 12 | 80 | 9.60 | lockfile present; secret or dependency scan tooling found |
| Code shape and semantic surface | 12 | 45 | 5.40 | largest authored code file: jnoccio-fusion/src/fusion.rs (3510 LOC); code file exceeds 500 LOC |
| Data truth and workflow safety | 8 | 60 | 4.80 | structured db boundary manifest present; db boundary routes roots, migrations, and constraints |
| Observability and repair evidence | 8 | 100 | 8.00 | observability libraries or patterns found; ops/observability directory present |
| Context economy and agent instructions | 7 | 93 | 6.51 | root `AGENTS.md` present; root `AGENTS.md` stays short |
| Jankurai tool adoption and CI replacement | 7 | 26 | 1.82 | control-plane files present; applicable=15 |
| Python containment and polyglot hygiene | 4 | 100 | 4.00 | no Python files in scope |
| Build speed signals | 4 | 30 | 1.20 | locked dependency graph present |

## Reference Profile Structure

- Applicable cells: `1` canonical=`1` noncanonical=`0` guidance missing=`0`

| Cell | Status | Canonical | Detected | Aliases | Guidance | Owner | Proof lane | Agent fix |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `web` | `not_applicable` | `apps/web/` | `-` | `frontend/, ui/, packages/web/, packages/ui/` | `not_required` | `apps/web` | `rendered UX / Playwright` | `no action` |
| `api` | `not_applicable` | `apps/api/` | `-` | `api/, server/, backend/` | `not_required` | `apps/api` | `edge handler / contract tests` | `no action` |
| `domain` | `not_applicable` | `crates/domain/` | `-` | `domain/, core/` | `not_required` | `crates/domain` | `unit / property tests` | `no action` |
| `application` | `not_applicable` | `crates/application/` | `-` | `application/, usecases/, use-cases/` | `not_required` | `crates/application` | `use-case / authz tests` | `no action` |
| `adapters` | `not_applicable` | `crates/adapters/` | `-` | `adapters/, infra/, integrations/` | `not_required` | `crates/adapters` | `adapter integration tests` | `no action` |
| `workers` | `not_applicable` | `crates/workers/` | `-` | `workers/, jobs/, scheduler/, queue/` | `not_required` | `crates/workers` | `workflow / replay tests` | `no action` |
| `contracts` | `not_applicable` | `contracts/` | `-` | `openapi/, protobuf/, json-schema/, generated/` | `not_required` | `contracts` | `generation / drift checks` | `no action` |
| `db` | `not_applicable` | `db/` | `-` | `migrations/, constraints/, sql/` | `not_required` | `db` | `migration / constraint tests` | `no action` |
| `python-ai` | `not_applicable` | `python/ai-service/` | `-` | `python/, ai-service/, evals/, embeddings/, model/` | `not_required` | `python/ai-service` | `eval / contract tests` | `no action` |
| `ops` | `canonical` | `ops/` | `.github, .github/workflows, ops` | `.github/, .github/workflows/, ci/, release/, observability/, security/` | `present` | `ops` | `security lane / workflow lint` | `keep `ops/AGENTS.md` aligned with owns / forbidden / proof lane guidance` |

## Rendered UX QA

- Web surface: `false`
- Layered UX lane: `true`
- Missing: `none`

## Tool Adoption

- Control plane present: `true`
- Applicable tools: `15`
- Configured: `12`
- CI evidence: `0`
- Artifact verified: `0`
- Replaced count: `0`
- Missing CI evidence: `audit-ci, proof-routing, proofbind, proofmark-rust, copy-code, security, ci-bad-behavior, git-bad-behavior, release-bad-behavior, contract-drift, rust-witness, authz-matrix, agent-tool-supply, release-readiness, cost-budget`

| Tool | Category | Mode | Status | Replaced | Artifacts |
| --- | --- | --- | --- | --- | --- |
| `audit-ci` | `audit` | `advisory` | `configured` | `manual repo scoring, ad hoc score gates` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `proof-routing` | `proof` | `advisory` | `configured` | `ad hoc proof lane selection, manual proof receipts` | `.jankurai/repo-score.json, .jankurai/repo-score.md, target/jankurai/repair-queue.jsonl` |
| `proofbind` | `proof` | `auto` | `configured` | `manual changed-surface routing, ad hoc proof obligation lists` | `target/jankurai/proofbind/surface-witness.json, target/jankurai/proofbind/obligations.json` |
| `proofmark-rust` | `proof` | `auto` | `configured` | `line-only coverage review, manual in-diff mutation review` | `target/jankurai/proofmark/proofmark-receipt.json, target/jankurai/proofmark/proof-receipt.json` |
| `copy-code` | `audit` | `auto` | `missing` | `ad hoc copy-code review, manual duplication triage` | `target/jankurai/copy-code.json, target/jankurai/copy-code.md` |
| `security` | `security` | `advisory` | `configured` | `gitleaks, dependency review, SBOM/provenance` | `target/jankurai/security/evidence.json` |
| `ci-bad-behavior` | `security` | `advisory` | `configured` | `mutable workflow refs, secret echo/debug workflow checks, non-blocking security scans` | `target/jankurai/language-bad-behavior.log` |
| `git-bad-behavior` | `audit` | `advisory` | `configured` | `destructive git automation, force-push release scripts, hidden stash-based state` | `target/jankurai/language-bad-behavior.log` |
| `release-bad-behavior` | `release` | `auto` | `configured` | `manual release checklist, ad hoc tag and artifact review, manual provenance review` | `target/jankurai/language-bad-behavior.log` |
| `ux-qa` | `ux` | `auto` | `not_applicable` | `playwright, axe-core, visual baselines` | `target/jankurai/ux-qa.json` |
| `db-migration-analyze` | `db` | `auto` | `not_applicable` | `manual migration review` | `target/jankurai/migration-report.json` |
| `contract-drift` | `contract` | `auto` | `missing` | `handwritten contract drift checks, openapi diff` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `rust-witness` | `rust` | `auto` | `configured` | `manual witness graphing` | `target/jankurai/rust/witness-graph.json` |
| `vibe-coverage` | `audit` | `auto` | `not_applicable` | `manual vibe-coding coverage spreadsheet` | `target/jankurai/vibe-coverage.json, target/jankurai/vibe-coverage.md` |
| `coverage-evidence` | `proof` | `auto` | `not_applicable` | `manual coverage report review, ad hoc mutation survivor review` | `target/jankurai/coverage/coverage-audit.json, target/jankurai/coverage/coverage-audit.md` |
| `authz-matrix` | `security` | `auto` | `missing` | `manual authz matrix review` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `input-boundary` | `security` | `auto` | `not_applicable` | `manual unsafe sink review` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `agent-tool-supply` | `security` | `advisory` | `configured` | `manual MCP/tool trust review` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `release-readiness` | `release` | `auto` | `configured` | `manual launch checklist` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `cost-budget` | `release` | `auto` | `configured` | `manual spend review` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |

## Boundary Reclassifications

No audited runtime boundary reclassifications declared.

## Findings

1. `medium` `shape` `.`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:shape` `soft` confidence `0.76`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: `Code shape and semantic surface` scored 45 below the standard floor of 85
   Fix: split large or ambiguous authored code into smaller semantic modules with focused tests
   Rerun: `just fast`
   Fingerprint: `sha256:d812a8441e548817f319c72bd4f48de7acd61a0768568fe61b6152893aa2a79e`
   Evidence: largest authored code file: jnoccio-fusion/src/fusion.rs (3510 LOC), code file exceeds 500 LOC, code file exceeds 1000 LOC, most code files stay under 300 LOC
2. `medium` `security` `.github/workflows/jankurai.yml`
   Rule: `HLT-016-SUPPLY-CHAIN-DRIFT`
   Check: `HLT-016-SUPPLY-CHAIN-DRIFT:security` `soft` confidence `0.76`
   Route: TLR `Security, secrets, agency`, lane `security`, owner `ci-release`
   Docs: `docs/audit-rubric.md#top-level-risk-mapping`
   Reason: `Security and supply-chain posture` scored 80 below the standard floor of 85
   Fix: wire secret, dependency, provenance, and workflow scans into an operational CI lane
   Rerun: `just security`
   Fingerprint: `sha256:3e21704bc51e05ff9b3194cbc4eea62a77cce19f572ea9056e29ca3ce474c7bd`
   Evidence: lockfile present, secret or dependency scan tooling found, provenance/SBOM tooling found, workflow linting tooling found
3. `medium` `proof` `Justfile`
   Rule: `HLT-018-PERF-CONCURRENCY-DRIFT`
   Check: `HLT-018-PERF-CONCURRENCY-DRIFT:proof` `soft` confidence `0.76`
   Route: TLR `Verification`, lane `fast`, owner `ci-release`
   Docs: `docs/testing.md`
   Reason: `Build speed signals` scored 30 below the standard floor of 85
   Fix: add fast deterministic build/test targets, caches, and narrow proof lanes for agent iteration
   Rerun: `just fast`
   Fingerprint: `sha256:5a2a647775dcb2bfabb2b25a7bc50806600cb3d9e0da806eb6ff5379b36f84c5`
   Evidence: locked dependency graph present
4. `medium` `data` `db/`
   Rule: `HLT-006-DIRECT-DB-WRONG-LAYER`
   Check: `HLT-006-DIRECT-DB-WRONG-LAYER:data` `soft` confidence `0.76`
   Route: TLR `Contracts/data`, lane `db`, owner `data`
   Docs: `docs/audit-rubric.md#required-shape`
   Reason: `Data truth and workflow safety` scored 60 below the standard floor of 85
   Fix: move durable truth into migrations, constraints, adapters, and application-owned transactions
   Rerun: `just fast`
   Fingerprint: `sha256:bc3c154999ceeadf008cf312a5b1205941d2a5bc9868961a1358cefa07b821ae`
   Evidence: structured db boundary manifest present, db boundary routes roots, migrations, and constraints

## Policy

- Policy file: `./agent/audit-policy.toml`
- Minimum score: `85`
- Fail on: ``

## Agent Fix Queue

1. `medium` `HLT-006-DIRECT-DB-WRONG-LAYER` `db/` - move durable truth into migrations, constraints, adapters, and application-owned transactions
   Route: `Contracts/data`/`db`
2. `medium` `HLT-018-PERF-CONCURRENCY-DRIFT` `Justfile` - add fast deterministic build/test targets, caches, and narrow proof lanes for agent iteration
   Route: `Verification`/`fast`
3. `medium` `HLT-001-DEAD-MARKER` `.` - split large or ambiguous authored code into smaller semantic modules with focused tests
   Route: `Entropy`/`fast`
4. `medium` `HLT-016-SUPPLY-CHAIN-DRIFT` `.github/workflows/jankurai.yml` - wire secret, dependency, provenance, and workflow scans into an operational CI lane
   Route: `Security, secrets, agency`/`security`
