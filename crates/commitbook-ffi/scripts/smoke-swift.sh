#!/usr/bin/env bash
# Compile, link, and RUN a small Swift consumer against the built xcframework.
#
# This is the end-to-end check that the framework is actually importable and
# that the `[Async]` UDL methods work across the real FFI boundary. It catches
# two classes of failure that `build-xcframework.sh` alone cannot:
#   1. A header/modulemap mismatch that still zips cleanly but fails to import.
#   2. An async method that traps because no tokio runtime is installed (the
#      UDL scaffolding polls futures on the caller's thread; see
#      crates/commitbook-ffi/src/runtime.rs).
#
# Requires: macOS with Xcode (swiftc), and a prior successful run of
#   ./crates/commitbook-ffi/scripts/build-xcframework.sh
#
# Usage:
#   ./crates/commitbook-ffi/scripts/smoke-swift.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

FRAMEWORK_NAME="CommitBookEngine"
LIB_NAME="commitbook_ffi"
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
defer { try? FileManager.default.removeItem(atPath: tmp) }
final class SmokeCredentials: GitCredentialCallback {
    func provide(request: GitCredentialRequest) -> GitCredentialResponse {
        return GitCredentialResponse(kind: .default, username: nil, password: nil,
                                     publicKey: nil, privateKey: nil, passphrase: nil,
                                     errorMessage: "No network credential expected in smoke test")
    }
}
let client = try! CommitBookEngineFfi(workspacesRoot: tmp,
                                         conflictResolver: nil,
                                         credentialCallback: SmokeCredentials())

// Synchronous method: proves the FFI boundary works at all.
let books = try! client.listCommitbooks()
guard books.isEmpty else {
    print("FAIL: expected an empty workspaces root, got \(books.count)")
    exit(1)
}
print("listCommitbooks -> 0 entries")

// Exercise the new registration and summary fields across generated Swift FFI.
func git(_ args: [String]) {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
    process.arguments = args
    try! process.run()
    process.waitUntilExit()
    precondition(process.terminationStatus == 0)
}
let clone = tmp + "/notes"
git(["init", clone])
git(["-C", clone, "remote", "add", "origin", "https://github.com/example/notes.git"])
try! FileManager.default.createDirectory(atPath: clone + "/.CommitBook", withIntermediateDirectories: true)
let config = """
[config]
schema = 1
[commitbook]
name = "Smoke notes"
[git]
remote = "origin"
branch = "main"
[sync]
schedule = "1h"
[commit]
mode = "timestamp"
agent = "any"
[conflicts]
mode = "manual"
agent = "claude"
"""
try! config.write(toFile: clone + "/.CommitBook/config.toml", atomically: true, encoding: .utf8)
let registered = try! client.registerLocalCommitbook(relativePath: "notes")
precondition(registered.commitbookLocalId.count == 8)
precondition(registered.remoteUrl == "https://github.com/example/notes.git")
precondition(try! client.getCommitbook(commitbookLocalId: registered.commitbookLocalId).commitbookLocalId == registered.commitbookLocalId)
print("registerLocalCommitbook -> persistent local identity")

// Async method: this traps if the shared tokio runtime is missing. A bogus id
// fails at the registry lookup, so no network is required.
let sem = DispatchSemaphore(value: 0)
var ok = false
Task {
    do {
        _ = try await client.syncCommitbook(commitbookLocalId: "does-not-exist",
                                            mode: .manual)
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
