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
carrier=$8
case "$major" in 17|18) ;; *) exit 2 ;; esac
[[ "$carrier" =~ ^docker\.io/williamluckyli/[a-z0-9._-]+@sha256:[0-9a-f]{64}$ ]]
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
(path / "postgres.commit").write_text(compute["postgres_source"] + "\n")
(path / "source-images.json").write_text(json.dumps({name: normal_release.immutable_reference(images[name])
                                                     for name in fixed}, sort_keys=True) + "\n")
PY

# 发行 registry 必须保留实际 OCI 的摘要。已有 tag 只能核对同一对象。
auth_file="${DOCKER_CONFIG:-$HOME/.docker}/config.json"
publish_archive() {
  local name=$1 artifact=$2 destination=$3 digest=$4 previous actual
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]
  if skopeo inspect --authfile "$auth_file" --raw "docker://$destination" > "$stage/$name-registry-before.json" 2> "$stage/$name-registry-before.stderr"; then
    previous=$(sha256sum "$stage/$name-registry-before.json" | cut -d' ' -f1)
    [[ "sha256:$previous" = "$digest" ]]
  else
    # 只有明确缺 manifest 才能首次写入同 attempt 的 tag。其他错误保留原文件。
    grep -Eiq 'manifest unknown|name unknown' "$stage/$name-registry-before.stderr"
    printf '%s\n' "$destination@$digest" > "$stage/$name-registry-push-intent"
    skopeo copy --dest-authfile "$auth_file" --all --preserve-digests \
      "oci-archive:$artifact" "docker://$destination" > "$stage/$name-registry-push.log"
  fi
  skopeo inspect --authfile "$auth_file" --raw "docker://$destination" > "$stage/$name-registry-after.json"
  actual=$(sha256sum "$stage/$name-registry-after.json" | cut -d' ' -f1)
  [[ "sha256:$actual" = "$digest" ]]
}

registry=$(jq -er .registry ci/distribution.json)
name="vm-compute-node-v$major"
base_name="normal-guest-base-v$major"
base_destination="$registry/$base_name:$tag"
mkdir "$stage/base-context" "$stage/publish" "$stage/images"
docker buildx build --builder "$buildx" --platform linux/amd64 --network=none --provenance=false \
  --build-arg "CARRIER_IMAGE=$carrier" \
  --build-arg "COMPUTE_IMAGE=$(jq -er --arg name "compute-node-v$major" '.[$name]' "$stage/source-images.json")" \
  --build-arg "DAEMON_IMAGE=$(jq -er '.["neonvm-daemon"]' "$stage/source-images.json")" \
  --build-arg "NEON_SOURCE=$(jq -er .neon_source "$stage/inputs.json")" \
  --build-arg "AUTOSCALING_SOURCE=$(jq -er .guest_autoscaling_source "$stage/inputs.json")" \
  --build-arg "POSTGRES_SOURCE=$(cat "$stage/postgres.commit")" --build-arg "POSTGRES_MAJOR=$major" \
  --build-arg "ORDINARY_PACKAGE_LOCK_SHA256=$(jq -er .normal_guest_inputs.ordinary_package_lock_sha256 ci/distribution.json)" \
  --file ci/normal-guest-base.Dockerfile --tag "$base_destination" --metadata-file "$stage/base-metadata.json" \
  --output "type=oci,dest=$stage/base.oci.tar" "$stage/base-context"
base_digest=$(jq -er '."containerimage.digest"' "$stage/base-metadata.json")
publish_archive base "$stage/base.oci.tar" "$base_destination" "$base_digest"
python3 ci/release.py record --name "$base_name" --image "$base_destination" --digest "$base_digest" \
  --output "$stage/publish/$base_name.json"
jq --arg base "$registry/$base_name@$base_digest" '.base_image=$base' "$stage/inputs.json" > "$stage/assembly-inputs.json"
cp -a "$artifacts/." "$stage/images/"
cp "$stage/publish/$base_name.json" "$stage/images/"

# 实际从同次发行锁定的 Builder 镜像提取工具。装配还会核对最终 Guest 内部字节。
builder=$(jq -er '."vm-builder"' "$stage/source-images.json")
docker pull "$builder"
container=$(docker create "$builder" true)
docker cp "$container:/usr/local/bin/vm-builder" "$stage/vm-builder"
docker rm "$container"
container=
chmod 0555 "$stage/vm-builder"
destination="$registry/$name:$tag"
python3 ci/pack_normal_guest.py --inputs "$stage/assembly-inputs.json" --vm-builder "$stage/vm-builder" \
  --ordinary-source-root "$ordinary" --autoscaling-source "$autoscaling" --output "$stage/assembly" \
  --buildx-builder "$buildx" --tag "$destination"
reference=$(jq -er .image "$stage/assembly/assembly-receipt.json")
digest="${reference##*@}"
[[ "$reference" = "$destination@$digest" && "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]

# 再从实际 registry 引用取盘核对。
publish_archive guest "$stage/assembly/guest.oci.tar" "$destination" "$digest"
docker pull "$reference"
container=$(docker create "$reference" true)
docker cp "$container:/disk.qcow2" "$stage/disk.qcow2"
docker rm "$container"
container=
python3 ci/release.py record --name "$name" --image "$destination" --digest "$digest" \
  --vm-disk "$stage/disk.qcow2" --vm-input-images "$stage/images" \
  --normal-guest "$stage/assembly" --output "$stage/publish/$name.json"
mv -T -- "$stage/publish" "$output"
