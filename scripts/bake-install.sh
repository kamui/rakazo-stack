#!/bin/sh
# Writes <dir>/install.sh for a release: this repo's install.sh with the release tag
# and the SHA-256 digests of the two executables in <dir> filled in.
#
#   scripts/bake-install.sh <tag> <dir>

set -eu

fail() {
    printf 'bake-install.sh: %s\n' "$*" >&2
    exit 1
}

[ $# -eq 2 ] || fail "usage: bake-install.sh <tag> <dir>"
tag=$1
dir=$2
case $tag in
*[!0-9A-Za-z.-]*) fail "$tag isn't a release tag" ;;
v[0-9]*) ;;
*) fail "$tag isn't a release tag" ;;
esac

digest() {
    [ -f "$1" ] || fail "$1 is missing"
    sha256sum <"$1" | cut -d ' ' -f 1
}

linux=$(digest "$dir/rakazo-stack-x86_64-unknown-linux-gnu")
macos=$(digest "$dir/rakazo-stack-aarch64-apple-darwin")

sed \
    -e "s/^release_tag='__RELEASE_TAG__'\$/release_tag='$tag'/" \
    -e "s/^sha256_linux_x86_64='__SHA256_LINUX_X86_64__'\$/sha256_linux_x86_64='$linux'/" \
    -e "s/^sha256_macos_arm64='__SHA256_MACOS_ARM64__'\$/sha256_macos_arm64='$macos'/" \
    "$(dirname "$0")/../install.sh" >"$dir/install.sh"

for filled in "release_tag='$tag'" "sha256_linux_x86_64='$linux'" "sha256_macos_arm64='$macos'"; do
    grep -qxF "$filled" "$dir/install.sh" || fail "install.sh has no placeholder for $filled"
done
