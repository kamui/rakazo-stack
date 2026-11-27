#!/bin/sh
# Installs the rakazo-stack program into ~/.local/bin, then starts `rakazo-stack setup`.
#
#   curl -fsSL https://github.com/kamui/rakazo-stack/releases/latest/download/install.sh | sh
#
# Add `-s -- --no-setup` after `sh` to install without starting setup.

# The braces make the shell read the whole script before it runs any of it, so a
# download cut off part way does nothing.
{

set -eu

# scripts/bake-install.sh fills these in when a release is built.
release_tag='__RELEASE_TAG__'
sha256_linux_x86_64='__SHA256_LINUX_X86_64__'
sha256_macos_arm64='__SHA256_MACOS_ARM64__'

repo='kamui/rakazo-stack'

say() {
    printf '%s\n' "$*"
}

stop() {
    printf 'Stop: %s\n' "$*" >&2
    exit 1
}

have() {
    command -v "$1" >/dev/null 2>&1
}

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    # Under Rosetta an Apple-silicon Mac reports x86_64.
    if [ "$os $arch" = 'Darwin x86_64' ] &&
        [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ]; then
        arch=arm64
    fi
    case "$os $arch" in
    'Linux x86_64')
        if [ -f /etc/alpine-release ] || ldd --version 2>&1 | grep -qi musl; then
            stop "Alpine/musl Linux isn't supported; use a glibc distro such as Ubuntu or Debian"
        fi
        target=x86_64-unknown-linux-gnu
        expected_sha256=$sha256_linux_x86_64
        ;;
    'Darwin arm64')
        target=aarch64-apple-darwin
        expected_sha256=$sha256_macos_arm64
        ;;
    *)
        stop "$os $arch isn't supported; rakazo-stack runs on x86-64 Linux (including WSL 2) and Apple-silicon Macs"
        ;;
    esac
}

check_release_values() {
    case $release_tag in
    v[0-9]*)
        case $expected_sha256 in
        *[!0-9a-f]*) ;;
        *) [ "${#expected_sha256}" -ne 64 ] || return 0 ;;
        esac
        ;;
    esac
    stop "this copy of install.sh isn't from a release, so it can't install anything; use https://github.com/$repo/releases/latest/download/install.sh"
}

download_program() {
    base="https://github.com/$repo/releases/download"
    protocols='=https'
    # For this repo's tests only: fetch from a fake release instead.
    if [ -n "${RAKAZO_STACK_TEST_DOWNLOAD_BASE:-}" ]; then
        base=$RAKAZO_STACK_TEST_DOWNLOAD_BASE
        protocols='=http,file'
    fi
    url="$base/$release_tag/rakazo-stack-$target"
    say "Downloading rakazo-stack $release_tag for $target..."
    curl -fsSL --proto "$protocols" -o "$download" "$url" </dev/null ||
        stop "couldn't download $url, so nothing was installed"
}

check_sha256() {
    actual_sha256=$(
        if have sha256sum; then
            sha256sum <"$download"
        else
            shasum -a 256 <"$download"
        fi | cut -d ' ' -f 1
    )
    [ "$actual_sha256" = "$expected_sha256" ] ||
        stop "the download doesn't match this install.sh (expected SHA-256 $expected_sha256, got $actual_sha256), so nothing was installed"
}

# A gh from before `gh attestation` can't check anything, so it counts as no gh.
check_build_origin() {
    origin_checked=no
    if have gh &&
        gh attestation verify --help </dev/null >/dev/null 2>&1 &&
        gh auth status --hostname github.com </dev/null >/dev/null 2>&1; then
        GH_HOST=github.com gh attestation verify "$download" --repo "$repo" </dev/null ||
            stop "gh couldn't verify where this download was built, so nothing was installed"
        origin_checked=yes
    fi
}

# The rename replaces the old program in one step, even while it's running.
install_program() {
    mkdir -p "$bin_dir"
    [ ! -d "$program" ] || stop "$program is a folder; move it away and run this again"
    staged=$(mktemp "$bin_dir/.rakazo-stack.XXXXXX")
    cp "$download" "$staged"
    chmod 755 "$staged"
    mv -f "$staged" "$program"
}

cleanup() {
    [ -z "$work_dir" ] || rm -rf "$work_dir"
    [ -z "$staged" ] || rm -f "$staged"
}

main() {
    run_setup=yes
    for argument in "$@"; do
        case $argument in
        --no-setup) run_setup=no ;;
        *) stop "unknown option $argument; the only option is --no-setup" ;;
        esac
    done
    [ -n "${HOME:-}" ] || stop "HOME isn't set"

    detect_target
    check_release_values
    have curl || stop "curl isn't installed"
    have sha256sum || have shasum || stop "neither sha256sum nor shasum is installed"

    bin_dir=$HOME/.local/bin
    program=$bin_dir/rakazo-stack
    work_dir=
    staged=
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' HUP TERM
    work_dir=$(mktemp -d)
    download=$work_dir/rakazo-stack

    download_program
    check_sha256
    check_build_origin
    install_program
    cleanup

    say "Installed rakazo-stack $release_tag at $program"
    if [ "$origin_checked" = no ]; then
        say "build origin not checked; to check it yourself: gh attestation verify \"$program\" --repo $repo"
    fi
    case ":${PATH:-}:" in
    *":$bin_dir:"* | *":$bin_dir/:"*) ;;
    *)
        say "$bin_dir isn't on your PATH. To run rakazo-stack by name, add this line to your shell profile:"
        # shellcheck disable=SC2016 # The line is for the profile, so it stays unexpanded.
        say '  export PATH="$HOME/.local/bin:$PATH"'
        ;;
    esac

    # /dev/tty exists even without a controlling terminal; only opening it tells.
    if [ "$run_setup" = yes ] && (: </dev/tty) 2>/dev/null; then
        exec "$program" setup </dev/tty
    fi
    say "Next: run \"$program\" setup"
}

main "$@"
}
