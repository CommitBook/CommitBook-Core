#!/usr/bin/env bash
#
# CommitBook installer
# Usage: curl -sSL https://raw.githubusercontent.com/zaai/commitbook/main/install.sh | bash
#

set -euo pipefail

REPO="zaai/commitbook"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BINARY_NAME="commitbook"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

info() {
    echo -e "${CYAN}${BOLD}==>${RESET}${BOLD} $1${RESET}"
}

success() {
    echo -e "${GREEN}${BOLD}==>${RESET}${BOLD} $1${RESET}"
}

error() {
    echo -e "${RED}${BOLD}Error:${RESET} $1" >&2
    exit 1
}

# Detect platform
detect_platform() {
    local os arch

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)  os="linux" ;;
        Darwin) os="macos" ;;
        *)      error "Unsupported OS: $os" ;;
    esac

    case "$arch" in
        x86_64|amd64)  arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *)             error "Unsupported architecture: $arch" ;;
    esac

    echo "${os}-${arch}"
}

# Check for required tools
check_deps() {
    if ! command -v git &>/dev/null; then
        error "git is required but not installed."
    fi

    if ! command -v cargo &>/dev/null; then
        info "Rust/Cargo not found. Installing from source requires Rust."
        info "Install Rust: https://rustup.rs/"
        info "Attempting to build from source..."
    fi
}

# Install from source using cargo
install_from_source() {
    info "Building CommitBook from source..."

    if ! command -v cargo &>/dev/null; then
        error "Cargo is required to build from source. Install Rust: https://rustup.rs/"
    fi

    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    info "Cloning repository..."
    git clone --depth 1 "https://github.com/${REPO}.git" "$tmpdir/commitbook" 2>/dev/null

    info "Building release binary..."
    cd "$tmpdir/commitbook"
    cargo build --release --quiet

    info "Installing to ${INSTALL_DIR}..."
    mkdir -p "$INSTALL_DIR"
    cp "target/release/${BINARY_NAME}" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/${BINARY_NAME}"

    success "CommitBook installed successfully!"
}

# Verify installation
verify_install() {
    if ! command -v "$BINARY_NAME" &>/dev/null; then
        # Check if INSTALL_DIR is in PATH
        if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
            echo ""
            info "Add ${INSTALL_DIR} to your PATH:"
            echo ""
            echo "  # Add to ~/.bashrc or ~/.zshrc:"
            echo "  export PATH=\"\$PATH:${INSTALL_DIR}\""
            echo ""
        fi
    fi

    if [ -x "$INSTALL_DIR/$BINARY_NAME" ]; then
        local version
        version="$("$INSTALL_DIR/$BINARY_NAME" --version 2>/dev/null || echo "unknown")"
        success "Installed: $version"
        echo ""
        echo "  Get started:"
        echo "    cd /path/to/your/notes"
        echo "    commitbook setup"
        echo "    commitbook doctor"
        echo "    commitbook start"
        echo ""
    fi
}

main() {
    echo ""
    echo -e "${CYAN}${BOLD}CommitBook Installer${RESET}"
    echo ""

    check_deps

    local platform
    platform="$(detect_platform)"
    info "Detected platform: $platform"

    install_from_source
    verify_install
}

main "$@"
