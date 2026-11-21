#!/usr/bin/env bash
# Preflight the exact upstream sources required by the full Compute extension profile.
# This runs once in the unified release prepare job, before expensive build matrices.
set -euo pipefail

readonly ORT_REPOSITORY="raymondxyang/onnxruntime"
readonly ORT_COMMIT="387127404e6c1d84b3468c387d864877ed1c67fe"
readonly ORT_TREE="4d3be68b9a81291f481a53b0850d99fed5007205"
readonly ORT_DEPS_BLOB="d213b09034f02524764ed54039d24ff51505edd9"
readonly PGRAG_URL="https://github.com/neondatabase-labs/pgrag/archive/refs/tags/v0.1.2.tar.gz"
readonly PGRAG_SHA256="7361654ea24f08cbb9db13c2ee1c0fe008f6114076401bb871619690dafc5225"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "[preflight] Verify ONNX Runtime 1.18.1 release commit and tree"
curl -fsSL --retry 5 "https://api.github.com/repos/${ORT_REPOSITORY}/git/commits/${ORT_COMMIT}" -o "$tmp/ort-commit.json"
python3 - "$tmp/ort-commit.json" "$ORT_COMMIT" "$ORT_TREE" <<'PY'
import json
import sys

document = json.load(open(sys.argv[1], encoding="utf-8"))
expected_commit, expected_tree = sys.argv[2:4]
if document.get("sha") != expected_commit:
    raise SystemExit("ONNX Runtime release commit SHA mismatch")
if document.get("tree", {}).get("sha") != expected_tree:
    raise SystemExit("ONNX Runtime release tree SHA mismatch")
PY

echo "[preflight] Verify the exact patched dependency manifest blob"
mkdir -p "$tmp/patch-root/cmake"
curl -fsSL --retry 5 "https://raw.githubusercontent.com/${ORT_REPOSITORY}/${ORT_COMMIT}/cmake/deps.txt" -o "$tmp/patch-root/cmake/deps.txt"
actual_blob="$(git hash-object "$tmp/patch-root/cmake/deps.txt")"
if [[ "$actual_blob" != "$ORT_DEPS_BLOB" ]]; then
  echo "ONNX Runtime cmake/deps.txt blob mismatch: $actual_blob" >&2
  exit 1
fi
patch --dry-run -p1 -d "$tmp/patch-root" < compute/patches/onnxruntime.patch

echo "[preflight] Verify pinned PGRAG v0.1.2 archive"
curl -fL --retry 5 "$PGRAG_URL" -o "$tmp/pgrag.tar.gz"
echo "$PGRAG_SHA256  $tmp/pgrag.tar.gz" | sha256sum --check --status

echo "[preflight] All pinned PGRAG and ONNX Runtime upstream sources are available and verified"
