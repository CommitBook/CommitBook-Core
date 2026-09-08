#!/usr/bin/env bash
set -euo pipefail

REPO="CommitBook/CommitBook-Core"
INSTALL_DIR="${HOME}/.local/bin"

# Verify a downloaded file against a SHA256SUMS manifest.
# The manifest lists every release target, so we filter it down to the one
# file we actually downloaded before handing it to the checksum tool.
verify_checksum() {
    local dir="$1" file="$2" sums="$3"
    local expected
    local -a sums_cmd

    expected="$(awk -v want="$file" '{ name = $2; sub(/^\*/, "", name); if (name == want) { print; exit } }' "$sums")"
    if [ -z "$expected" ]; then
        echo "No checksum for ${file} in SHA256SUMS" >&2
        exit 1
    fi

    printf '%s\n' "$expected" > "${dir}/SHA256SUMS.filtered"

    if command -v sha256sum >/dev/null 2>&1; then
        sums_cmd=(sha256sum -c SHA256SUMS.filtered)
    elif command -v shasum >/dev/null 2>&1; then
        sums_cmd=(shasum -a 256 -c SHA256SUMS.filtered)
    else
        echo "Neither sha256sum nor shasum found; cannot verify download" >&2
        exit 1
    fi

    if ! (cd "$dir" && "${sums_cmd[@]}" >/dev/null 2>&1); then
        echo "Checksum mismatch for ${file}" >&2
        echo "Expected: ${expected}" >&2
        echo "The download may be corrupt or tampered with. Aborting." >&2
        exit 1
    fi

    echo "Checksum verified: ${file}"
}

main() {
    local os arch target

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Darwin)
            case "$arch" in
                x86_64)  target="x86_64-apple-darwin" ;;
                arm64)   target="aarch64-apple-darwin" ;;
                *)       echo "Unsupported architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        Linux)
            case "$arch" in
                x86_64)  target="x86_64-unknown-linux-gnu" ;;
                aarch64) target="aarch64-unknown-linux-gnu" ;;
                *)       echo "Unsupported architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        *)
            echo "Unsupported OS: $os" >&2
            exit 1
            ;;
    esac

    echo "Detecting platform: ${target}"

    # Get latest release tag
    local tag
    tag="$(curl -sS "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep '"tag_name"' | head -1 | cut -d'"' -f4)"

    if [ -z "$tag" ]; then
        echo "Failed to fetch latest release tag" >&2
        exit 1
    fi

    echo "Latest release: ${tag}"

    local base="https://github.com/${REPO}/releases/download/${tag}"
    local archive="commitbook-${target}.tar.gz"
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    echo "Downloading ${base}/${archive}..."
    curl -fsSL "${base}/${archive}" -o "${tmp}/${archive}"

    echo "Downloading ${base}/SHA256SUMS..."
    curl -fsSL "${base}/SHA256SUMS" -o "${tmp}/SHA256SUMS"

    verify_checksum "$tmp" "$archive" "${tmp}/SHA256SUMS"

    echo "Extracting to ${INSTALL_DIR}..."
    mkdir -p "$INSTALL_DIR"
    tar xzf "${tmp}/${archive}" -C "$INSTALL_DIR"

    # Verify
    if command -v "${INSTALL_DIR}/commitbook" &>/dev/null; then
        echo ""
        echo "Installed successfully!"
        "${INSTALL_DIR}/commitbook" --version
    else
        echo ""
        echo "Installed to ${INSTALL_DIR}/commitbook"
    fi

    # Check PATH
    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
        echo ""
        echo "Add ${INSTALL_DIR} to your PATH:"
        echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    fi
}

main "$@"
