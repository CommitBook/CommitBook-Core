#!/usr/bin/env bash
# CommitBook - Conductor Run Script
# Builds the CLI and demonstrates available commands.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$PROJECT_DIR"

# Always rebuild to pick up local changes
echo "Building..."
cargo build -p commitbook-cli
echo ""

BINARY="$PROJECT_DIR/target/debug/commitbook"

echo "========================================"
echo "  CommitBook CLI"
echo "========================================"
echo ""
echo "Binary: $BINARY"
echo "Version: $("$BINARY" --version)"
echo ""

# Show built-in help
"$BINARY" --help
echo ""

echo "========================================"
echo "  Quick Start"
echo "========================================"
echo ""
echo "Initialize CommitBook in any git repository:"
echo ""
echo "  cd /path/to/your/repo"
echo "  $BINARY init            # Initialize .CommitBook/ (once per repo)"
echo "  $BINARY doctor          # Check system health"
echo "  $BINARY sync            # Commit and push changes"
echo "  $BINARY start           # Start the sync scheduler"
echo "  $BINARY status          # Show sync state"
echo "  $BINARY schedule daily  # Change sync frequency"
echo "  $BINARY log             # View recent activity"
echo "  $BINARY stop            # Stop the scheduler"
echo ""
echo "Note: run 'commitbook init' once per repo before any other command."
echo ""

echo "========================================"
echo "  Add to PATH"
echo "========================================"
echo ""
echo "To use 'commitbook' directly in your terminal, run:"
echo ""
echo "  export PATH=\"$PROJECT_DIR/target/debug:\$PATH\""
echo ""
echo "Add that line to ~/.zshrc (or ~/.bashrc) to make it permanent."
echo ""
