# ops

- Keep CI wrappers deterministic and shell-only.
- Do not add nonblocking security or proof jobs.
- Keep GitHub and GitLab lanes dispatching to the same `ops/ci/*.sh` scripts.
