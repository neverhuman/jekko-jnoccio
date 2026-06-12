#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
helper="$repo_root/packages/jekko/script/jnoccio-install-bundle.mjs"

node --input-type=module - "$helper" <<'NODE'
import { pathToFileURL } from "node:url"

const helperPath = process.argv[2]
const { seedJnoccioFusionBundle } = await import(pathToFileURL(helperPath).href)

const result = seedJnoccioFusionBundle()
console.log(
  [
    `Bundle root: ${result.bundleDir}`,
    result.bundleCreated ? "Seeded new bundle files" : "Existing bundle left unchanged",
  ].join("\n"),
)
NODE
echo "Installed Jnoccio bundle under ${HOME}/.config/jekko/jnoccio-fusion"
