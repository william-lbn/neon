#!/usr/bin/env bash
set -euo pipefail

task_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
task_version='1.7.12'
task_sha256='8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8'

# 契约门禁使用 Linux amd64 runner；固定平台和发布摘要，避免静默下载另一种二进制。
# 更新版本时必须同时核对官方 release 的 linux_amd64 资产摘要。
if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
    echo 'Workflow syntax checks require Linux amd64' >&2
    exit 1
fi

# 下载和解包均放在独占临时目录；成功、失败和退出时清理，不污染源码或复用旧缓存。
task_dir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/neon-actionlint.XXXXXX")"
trap 'rm -rf -- "$task_dir"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
task_archive="$task_dir/actionlint.tar.gz"
curl --fail --location --silent --show-error --retry 3 --retry-delay 2 \
    --connect-timeout 15 --max-time 120 --proto '=https' --tlsv1.2 \
    "https://github.com/rhysd/actionlint/releases/download/v${task_version}/actionlint_${task_version}_linux_amd64.tar.gz" \
    --output "$task_archive"
printf '%s  %s\n' "$task_sha256" "$task_archive" | sha256sum --check --strict --status
tar -xzf "$task_archive" -C "$task_dir" actionlint

# 本门禁检查工作流语法、表达式和接口；显式关闭可选工具，保证 runner 是否预装
# ShellCheck/Pyflakes 不改变检查范围，也不将此结果误当成 shell/Python 运行测试。
cd -- "$task_root"
"$task_dir/actionlint" -version
"$task_dir/actionlint" -shellcheck='' -pyflakes=''
