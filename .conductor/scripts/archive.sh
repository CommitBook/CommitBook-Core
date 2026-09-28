#!/usr/bin/env bash
# CommitBook - Conductor Archive Script
# Cleans build artifacts to reclaim disk space.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$PROJECT_DIR"

echo "========================================"
echo "  CommitBook CLI - Archive"
echo "========================================"
echo ""
echo "Project directory: $PROJECT_DIR"
echo ""

if ! command -v cargo &>/dev/null; then
    echo "[ERROR] cargo not found. Cannot clean build artifacts."
    exit 1
fi

echo "--- Cleaning build artifacts ---"
cargo clean
echo ""

echo "========================================"
echo "  Archive Complete"
echo "========================================"
echo ""
echo "Build artifacts removed. Run ./.conductor/scripts/setup.sh to rebuild."
echo ""
