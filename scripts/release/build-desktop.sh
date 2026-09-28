#!/usr/bin/env bash
# Shared by CI build checks and release packaging. Run from the repository root.
set -euo pipefail
TARGET="${1:?Rust target is required}"
USE_CROSS="${2:-false}"
BUILDER=cargo
case "$USE_CROSS" in
  true) BUILDER=cross ;;
  false) ;;
  *) echo "Expected use-cross to be true or false" >&2; exit 1 ;;
esac
"$BUILDER" build --release --locked --target "$TARGET" \
  -p commitbook-cli -p commitbook-tui -p commitbook-web \
  --features commitbook-engine/vendored-tls
