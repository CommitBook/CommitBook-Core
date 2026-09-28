#!/usr/bin/env bash
# Compile, link, and RUN a small Swift consumer against the built xcframework.
#
# This is the end-to-end check that the framework is actually importable and
# that the `[Async]` UDL methods work across the real FFI boundary. It catches
# two classes of failure that `build-xcframework.sh` alone cannot:
#   1. A header/modulemap mismatch that still zips cleanly but fails to import.
#   2. An async method that traps because no tokio runtime is installed (the
#      UDL scaffolding polls futures on the caller's thread; see
#      crates/commitbook-client/src/runtime.rs).
#
# Requires: macOS with Xcode (swiftc), and a prior successful run of
#   ./crates/commitbook-client/scripts/build-xcframework.sh
#
# Usage:
#   ./crates/commitbook-client/scripts/smoke-swift.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

FRAMEWORK_NAME="CommitBookEngine"
LIB_NAME="commitbook_client"
XC="$REPO_ROOT/target/xcframework/${FRAMEWORK_NAME}.xcframework"
SLICE="macos-arm64_x86_64"

[ -d "$XC" ] || {
    echo "error: $XC not found. Run scripts/build-xcframework.sh first." >&2
    exit 1
}

HEADERS="$XC/$SLICE/Headers"
LIBDIR="$XC/$SLICE"
[ -d "$HEADERS" ] || { echo "error: missing $HEADERS" >&2; exit 1; }

# The generated Swift API surface is bundled into the framework by step 7 of
# build-xcframework.sh; a real consumer compiles it into its own target.
SWIFT_SRC=$(find "$XC/Sources" -name '*.swift' | head -n1)
[ -n "$SWIFT_SRC" ] || { echo "error: no generated .swift in $XC/Sources" >&2; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cp "$SWIFT_SRC" "$WORK/"

cat > "$WORK/main.swift" <<'SWIFT'
import Foundation

let tmp = NSTemporaryDirectory() + "cb-smoke-\(UUID().uuidString)"
let client = try! CommitBookEngineClient(workspacesRoot: tmp,
                                         conflictResolver: nil)

// Synchronous method: proves the FFI boundary works at all.
let books = try! client.listCommitbooks()
guard books.isEmpty else {
    print("FAIL: expected an empty workspaces root, got \(books.count)")
    exit(1)
}
print("listCommitbooks -> 0 entries")

// Async method: this traps if the shared tokio runtime is missing. A bogus id
// fails at the registry lookup, so no network is required.
let sem = DispatchSemaphore(value: 0)
var ok = false
Task {
    do {
        _ = try await client.syncCommitbook(commitbookId: "does-not-exist",
                                            mode: .manual,
                                            token: "token")
        print("FAIL: expected a NotFound error")
    } catch {
        print("syncCommitbook -> expected error: \(error)")
        ok = true
    }
    sem.signal()
}
sem.wait()

guard ok else { exit(1) }
print("SMOKE OK")
SWIFT

echo ">> Building Swift smoke test against $SLICE"
swiftc -O \
    -I "$HEADERS" \
    "$WORK/$(basename "$SWIFT_SRC")" "$WORK/main.swift" \
    -L "$LIBDIR" -l"$LIB_NAME" \
    -lz -liconv \
    -framework Security -framework SystemConfiguration -framework CoreFoundation \
    -o "$WORK/smoke"

echo ">> Running"
"$WORK/smoke"
