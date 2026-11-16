#!/usr/bin/env bash
# 组装已经由同次发行提供原镜像记录的 PG17/PG18 normal Guest。
set -Eeuo pipefail

major=$1
tag=$2
artifacts=$3
autoscaling=$4
ordinary=$5
buildx=$6
output=$7
case "$major" in 17|18) ;; *) exit 2 ;; esac
python3 ci/dsga_release.py verify-attempt --tag "$tag" --arch amd64
[[ ! -e "$output" ]]
artifacts=$(realpath "$artifacts")
autoscaling=$(realpath "$autoscaling")
ordinary=$(realpath "$ordinary")
mkdir -p "$(dirname "$output")"
stage="$output.pending"
mkdir -m 0700 -- "$stage"
container=
cleanup() {
  status=$?
  if [[ -n "$container" ]]; then docker rm "$container" >&2 || true; fi
  if [[ "$status" = 0 ]]; then
    rm -rf -- "$stage"
  else
    printf 'Normal Guest attempt evidence preserved at %s\n' "$stage" >&2
  fi
  exit "$status"
}
trap cleanup EXIT

python3 - "$artifacts" "$major" "$tag" "$stage" <<'PY'
import json
import pathlib
import subprocess
import sys

sys.path.insert(0, "ci")
import normal_release
import release

records, major, tag, stage = sys.argv[1:]
version = "v" + major
images = release.load_images(records)
execution = release.validate_current_execution(tag)
source = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
auto = release.LOCK["autoscaling_commit"]
fixed = {
    "compute-node-" + version: ("william-lbn/neon", source),
    "normal-guest-base-" + version: ("william-lbn/neon", source),
    "vm-kernel-normal": ("william-lbn/autoscaling", auto),
    "vm-builder": ("william-lbn/autoscaling", auto),
    "neonvm-daemon": ("william-lbn/autoscaling", auto),
    "neonvm-runner": ("william-lbn/autoscaling", auto),
}
for name, (repo, commit) in fixed.items():
    image = images[name]
    normal_release.same(image, {
        "name": name, "kind": "runtime", "execution": execution, "architecture": "linux/amd64",
        "source_repository": repo, "source_commit": commit,
        "image": f"{release.LOCK['registry']}/{name}:{tag}",
    }, f"Normal Guest material is outside the current run: {name}")
    normal_release.require(normal_release.digest(image.get("digest")), f"Missing OCI digest: {name}")
compute = images["compute-node-" + version]
normal_release.require(compute.get("postgres_source") == release.LOCK["postgres"][version],
                       "Normal Guest compute image was not built with the locked PostgreSQL")
daemon = images["neonvm-daemon"]
kernel = images["vm-kernel-normal"]
builder = images["vm-builder"]
inputs = {
    "postgres_major": int(major),
    "base_image": normal_release.immutable_reference(images["normal-guest-base-" + version]),
    "disk_tools_image": normal_release.immutable_reference(images["neonvm-runner"]),
    "kernel_image": normal_release.immutable_reference(kernel),
    "kernel_sha256": kernel["artifact_sha256"],
    "builder_sha256": builder["builder_sha256"],
    "postgres_elf_sha256": compute["postgres_elf_sha256"],
    "neon_source": source,
    "guest_autoscaling_source": auto,
    "runtime_elf_sha256": {"compute_ctl": compute["runtime_elf_sha256"]["compute_ctl"],
                           "neonvmd": daemon["runtime_elf_sha256"]["neonvmd"],
                           "dsga_resource_guard": daemon["runtime_elf_sha256"]["dsga_resource_guard"]},
}
path = pathlib.Path(stage)
(path / "inputs.json").write_text(json.dumps(inputs, sort_keys=True) + "\n")
(path / "source-images.json").write_text(json.dumps({name: normal_release.immutable_reference(images[name])
                                                     for name in fixed}, sort_keys=True) + "\n")
PY

# 实际从同次发行锁定的 Builder 镜像提取工具。装配还会核对最终 Guest 内部字节。
builder=$(jq -er '."vm-builder"' "$stage/source-images.json")
docker pull "$builder"
container=$(docker create "$builder" true)
docker cp "$container:/usr/local/bin/vm-builder" "$stage/vm-builder"
docker rm "$container"
container=
chmod 0555 "$stage/vm-builder"
registry=$(jq -er .registry ci/distribution.json)
name="vm-compute-node-v$major"
destination="$registry/$name:$tag"
python3 ci/pack_normal_guest.py --inputs "$stage/inputs.json" --vm-builder "$stage/vm-builder" \
  --ordinary-source-root "$ordinary" --autoscaling-source "$autoscaling" --output "$stage/assembly" \
  --buildx-builder "$buildx" --tag "$destination"
reference=$(jq -er .image "$stage/assembly/assembly-receipt.json")
digest="${reference##*@}"
[[ "$reference" = "$destination@$digest" && "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]

# 发行 registry 必须保留经独立读回的 OCI 摘要。再从实际 registry 引用取盘核对。
auth_file="${DOCKER_CONFIG:-$HOME/.docker}/config.json"
if skopeo inspect --authfile "$auth_file" --raw "docker://$destination" > "$stage/registry-before.json" 2> "$stage/registry-before.stderr"; then
  previous=$(sha256sum "$stage/registry-before.json" | cut -d' ' -f1)
  [[ "sha256:$previous" = "$digest" ]]
else
  # 只有明确缺 manifest 才能首次写入同 attempt 的 tag。其他错误保留原文件。
  grep -Eiq 'manifest unknown|name unknown' "$stage/registry-before.stderr"
  printf '%s\n' "$reference" > "$stage/registry-push-intent"
  skopeo copy --dest-authfile "$auth_file" --all --preserve-digests \
    "oci-archive:$stage/assembly/guest.oci.tar" "docker://$destination" > "$stage/registry-push.log"
fi
skopeo inspect --authfile "$auth_file" --raw "docker://$destination" > "$stage/registry-after.json"
actual=$(sha256sum "$stage/registry-after.json" | cut -d' ' -f1)
[[ "sha256:$actual" = "$digest" ]]
docker pull "$reference"
container=$(docker create "$reference" true)
docker cp "$container:/disk.qcow2" "$stage/disk.qcow2"
docker rm "$container"
container=
python3 ci/release.py record --name "$name" --image "$destination" --digest "$digest" \
  --vm-disk "$stage/disk.qcow2" --vm-input-images "$artifacts" \
  --normal-guest "$stage/assembly" --output "$stage/image.json"
ln -T -- "$stage/image.json" "$output"
