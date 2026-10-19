#!/bin/sh
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
sys="$(uname -s)"
case "$sys" in
MINGW* | MSYS* | CYGWIN*) rel="x86_64-pc-windows-msvc/gray-achieve.exe" ;;
Darwin*)
    case "$(uname -m)" in
    arm64) rel="aarch64-apple-darwin/gray-achieve" ;;
    *) rel="x86_64-apple-darwin/gray-achieve" ;;
    esac ;;
*)
    case "$(uname -m)" in
    aarch64 | arm64) rel="aarch64-unknown-linux-musl/gray-achieve" ;;
    *) rel="x86_64-unknown-linux-musl/gray-achieve" ;;
    esac ;;
esac
if [ -x "$here/$rel" ]; then exec "$here/$rel" "$@"; fi
for cand in "$here/target/release/gray-achieve.exe" "$here/target/release/gray-achieve" "$here/target/debug/gray-achieve.exe" "$here/target/debug/gray-achieve"; do
    if [ -x "$cand" ]; then exec "$cand" "$@"; fi
done
echo "gray-achieve binary not found in $here" >&2
exit 1
