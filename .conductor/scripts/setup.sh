#!/usr/bin/env bash
# CommitBook - Conductor Setup Script
# Checks prerequisites and builds the CLI binary.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$PROJECT_DIR"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "========================================"
echo "  CommitBook CLI - Setup"
echo "========================================"
echo ""
echo "Project directory: $PROJECT_DIR"
echo ""

# --- Check Rust toolchain ---
echo "--- Checking Rust Toolchain ---"
if ! command -v rustc &>/dev/null; then
    echo -e "${RED}[ERROR]${NC} Rust is not installed."
    echo ""
    echo "  Install Rust via rustup:"
    echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo ""
    echo "  Then restart your shell and re-run this script."
    exit 1
fi
echo -e "${GREEN}[OK]${NC} rustc: $(rustc --version)"

if ! command -v cargo &>/dev/null; then
    echo -e "${RED}[ERROR]${NC} cargo not found (should be installed with Rust)."
    exit 1
fi
echo -e "${GREEN}[OK]${NC} cargo: $(cargo --version)"
echo ""

# --- Check Git (required by commitbook at runtime) ---
echo "--- Checking Git ---"
if ! command -v git &>/dev/null; then
    echo -e "${YELLOW}[WARN]${NC} git not found. CommitBook requires git at runtime."
    echo "  Install git: https://git-scm.com/downloads"
else
    echo -e "${GREEN}[OK]${NC} git: $(git --version)"
fi
echo ""

# --- Build the CLI ---
echo "--- Building CommitBook CLI ---"
BINARY="$PROJECT_DIR/target/debug/commitbook"
if [ -f "$BINARY" ]; then
    echo "Removing old binary..."
    rm "$BINARY"
fi
echo "Running: cargo build -p commitbook-cli"
echo ""
cargo build -p commitbook-cli
echo ""

if [ ! -x "$BINARY" ]; then
    echo -e "${RED}[ERROR]${NC} Build succeeded but binary not found at: $BINARY"
    exit 1
fi
echo -e "${GREEN}[OK]${NC} Binary: $BINARY"
echo ""

echo "========================================"
echo "  Setup Complete"
echo "========================================"
echo ""
echo "Next steps:"
echo "  Run demo:   ./.conductor/scripts/run.sh"
echo "  Run tests:  cargo test"
echo ""
