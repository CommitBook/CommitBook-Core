#!/usr/bin/env bash
set -euo pipefail

REPO="ZAAI-com/CommitBook"
INSTALL_DIR="${HOME}/.local/bin"

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

    local url="https://github.com/${REPO}/releases/download/${tag}/commitbook-${target}.tar.gz"
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    echo "Downloading ${url}..."
    curl -sLS "$url" -o "${tmp}/commitbook.tar.gz"

    echo "Extracting to ${INSTALL_DIR}..."
    mkdir -p "$INSTALL_DIR"
    tar xzf "${tmp}/commitbook.tar.gz" -C "$INSTALL_DIR"

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
