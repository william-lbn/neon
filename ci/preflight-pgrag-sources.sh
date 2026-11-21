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

echo "[preflight] Fetch and verify ONNX Runtime 1.18.1 release commit"
git init -q "$tmp/onnxruntime"
git -C "$tmp/onnxruntime" remote add origin "https://github.com/${ORT_REPOSITORY}.git"
git -C "$tmp/onnxruntime" fetch --depth 1 origin "$ORT_COMMIT"
git -C "$tmp/onnxruntime" checkout --detach FETCH_HEAD
actual_commit="$(git -C "$tmp/onnxruntime" rev-parse HEAD)"
actual_tree="$(git -C "$tmp/onnxruntime" rev-parse 'HEAD^{tree}')"
actual_blob="$(git -C "$tmp/onnxruntime" rev-parse "HEAD:cmake/deps.txt")"
[[ "$actual_commit" == "$ORT_COMMIT" ]]
[[ "$actual_tree" == "$ORT_TREE" ]]
[[ "$actual_blob" == "$ORT_DEPS_BLOB" ]]
patch --dry-run -p1 -d "$tmp/onnxruntime" < compute/patches/onnxruntime.patch
echo "[preflight] Verify pinned PGRAG v0.1.2 archive"
curl -fL --retry 5 "$PGRAG_URL" -o "$tmp/pgrag.tar.gz"
echo "$PGRAG_SHA256  $tmp/pgrag.tar.gz" | sha256sum --check --status

echo "[preflight] All pinned PGRAG and ONNX Runtime upstream sources are available and verified"
