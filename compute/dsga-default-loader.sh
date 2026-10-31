#!/bin/bash
# 仅构建期固定加载器布局。运行时原Source仍须核所有RO祖先、ELF闭包和准确hash。
set -euo pipefail
test "$#" = 3
test "$1" = default-loader-v1
test "$2" = v17
test "$3" = amd64
test "$(id -u)" = 0
test -f /usr/local/bin/postgres
test ! -e /etc/ld.so.preload
test ! -L /etc/ld.so.preload

# 原ldconfig优先选择/usr/local/lib的Neon libpq等。移除cache后，默认目录必须
# 仍选择同字节的核心ABI；复制到独立只读核心目录，不能指向postgres可替换的目录。
core=/usr/lib/x86_64-linux-gnu/neon-pg17-core
test ! -e "$core"
test ! -L "$core"
install -d -o root -g root -m 0755 "$core"
count=0
for file in /usr/local/lib/lib*.so*; do
    test -f "$file"
    name="${file##*/}"
    case "$name" in *[!a-zA-Z0-9_.+-]*|'') exit 1 ;; esac
    install -o root -g root -m 0644 "$(readlink -f "$file")" "$core/$name"
    ln -sfn "$core/$name" "/usr/lib/x86_64-linux-gnu/$name"
    count=$((count+1))
done
test "$count" -gt 0

# 默认loader的cache/preload/hwcaps不能成为未声明的替代来源。只拒实际不支持
# 的目录；不删除库或伪造来源观察。不更改宿主机，仅处理本层镜像的cache文件。
for dir in /lib /usr/lib /lib64 /usr/lib64 /lib/x86_64-linux-gnu /usr/lib/x86_64-linux-gnu; do
    test ! -e "$dir/glibc-hwcaps"
    test ! -L "$dir/glibc-hwcaps"
done
rm -f /etc/ld.so.cache
test ! -e /etc/ld.so.cache
test ! -L /etc/ld.so.cache
