#!/usr/bin/env bash
# Build CommitBookEngine.xcframework for iOS + macOS slices and emit
# a zipped artifact + SHA256 ready to attach to a GitHub release.
#
# Output:
#   target/CommitBookEngine.xcframework.zip
#   target/CommitBookEngine.xcframework.zip.sha256
#
# Required toolchain:
#   - Xcode (xcodebuild) 15+
#   - Rust targets: aarch64-apple-ios, aarch64-apple-ios-sim,
#     x86_64-apple-ios, aarch64-apple-darwin, x86_64-apple-darwin
#
# Run from anywhere:
#   ./crates/commitbook-client/scripts/build-xcframework.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

CRATE="commitbook-client"
LIB_NAME="commitbook_client"
FRAMEWORK_NAME="CommitBookEngine"
SWIFT_MODULE="CommitBookEngineFFI"

OUT_DIR="$REPO_ROOT/target/xcframework"
BINDINGS_DIR="$OUT_DIR/Bindings"
ASSEMBLY_DIR="$OUT_DIR/staging"

rm -rf "$OUT_DIR"
mkdir -p "$BINDINGS_DIR" "$ASSEMBLY_DIR"

# iOS deployment target, match what libgit2-sys ships (ios 14+ on
# modern Xcode). Override via env if needed.
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-15.0}"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"

build_target() {
    local target="$1"
    echo ">> Building $target"
    cargo build --release --target "$target" -p "$CRATE"
}

# 1. Build all five slices.
build_target aarch64-apple-ios
build_target aarch64-apple-ios-sim
build_target x86_64-apple-ios
build_target aarch64-apple-darwin
build_target x86_64-apple-darwin

# 2. Generate Swift bindings from the compiled library's metadata.
echo ">> Generating Swift bindings"
cargo run --release --target aarch64-apple-darwin --bin uniffi-bindgen -p "$CRATE" -- \
    generate \
    --library "target/aarch64-apple-darwin/release/lib${LIB_NAME}.a" \
    --language swift \
    --out-dir "$BINDINGS_DIR" \
    --no-format

# 3. Lipo the simulator slices (arm64 + x86_64) into one fat archive.
echo ">> Lipo simulator slices"
mkdir -p "$ASSEMBLY_DIR/ios-sim-fat"
lipo -create \
    "target/aarch64-apple-ios-sim/release/lib${LIB_NAME}.a" \
    "target/x86_64-apple-ios/release/lib${LIB_NAME}.a" \
    -output "$ASSEMBLY_DIR/ios-sim-fat/lib${LIB_NAME}.a"

# 4. Lipo macOS slices.
echo ">> Lipo macOS slices"
mkdir -p "$ASSEMBLY_DIR/macos-fat"
lipo -create \
    "target/aarch64-apple-darwin/release/lib${LIB_NAME}.a" \
    "target/x86_64-apple-darwin/release/lib${LIB_NAME}.a" \
    -output "$ASSEMBLY_DIR/macos-fat/lib${LIB_NAME}.a"

# 5. Stage headers + modulemap per slice. UniFFI emits files named after
#    the UDL namespace (`commitbook`); rename to the framework module
#    name for xcframework consumption.
stage_headers() {
    local dir="$1"
    mkdir -p "$dir"
    # UniFFI names the header/modulemap after the UDL namespace (commitbook),
    # not the Swift module. Discover them and fail loudly if bindgen did not
    # emit them, rather than silently producing a header-less framework.
    local hdr modmap
    hdr=$(find "$BINDINGS_DIR" -name '*FFI.h' | head -n1)
    modmap=$(find "$BINDINGS_DIR" -name '*FFI.modulemap' | head -n1)
    [ -n "$hdr" ] || { echo "error: no generated *FFI.h in $BINDINGS_DIR" >&2; exit 1; }
    [ -n "$modmap" ] || { echo "error: no generated *FFI.modulemap in $BINDINGS_DIR" >&2; exit 1; }
    cp "$hdr" "$dir/${SWIFT_MODULE}.h"
    cp "$modmap" "$dir/module.modulemap"
}

mkdir -p "$ASSEMBLY_DIR/ios-device-headers" \
         "$ASSEMBLY_DIR/ios-sim-headers" \
         "$ASSEMBLY_DIR/macos-headers"
stage_headers "$ASSEMBLY_DIR/ios-device-headers"
stage_headers "$ASSEMBLY_DIR/ios-sim-headers"
stage_headers "$ASSEMBLY_DIR/macos-headers"

# 6. Build the xcframework.
echo ">> Creating xcframework"
xcodebuild -create-xcframework \
    -library "target/aarch64-apple-ios/release/lib${LIB_NAME}.a" \
    -headers "$ASSEMBLY_DIR/ios-device-headers" \
    -library "$ASSEMBLY_DIR/ios-sim-fat/lib${LIB_NAME}.a" \
    -headers "$ASSEMBLY_DIR/ios-sim-headers" \
    -library "$ASSEMBLY_DIR/macos-fat/lib${LIB_NAME}.a" \
    -headers "$ASSEMBLY_DIR/macos-headers" \
    -output "$OUT_DIR/${FRAMEWORK_NAME}.xcframework"

# 7. Bundle Swift sources alongside the framework. Apple-side
#    consumers add these to their target.
mkdir -p "$OUT_DIR/${FRAMEWORK_NAME}.xcframework/Sources"
cp "$BINDINGS_DIR"/*.swift "$OUT_DIR/${FRAMEWORK_NAME}.xcframework/Sources/" 2>/dev/null || true

# 8. Zip.
echo ">> Zipping"
cd "$OUT_DIR"
zip -r -X "${FRAMEWORK_NAME}.xcframework.zip" "${FRAMEWORK_NAME}.xcframework" >/dev/null
mv "${FRAMEWORK_NAME}.xcframework.zip" "$REPO_ROOT/target/"

# 9. SHA256.
cd "$REPO_ROOT/target"
shasum -a 256 "${FRAMEWORK_NAME}.xcframework.zip" \
    | awk '{print $1}' > "${FRAMEWORK_NAME}.xcframework.zip.sha256"

echo ""
echo "=== Build complete ==="
echo "  Artifact:  $REPO_ROOT/target/${FRAMEWORK_NAME}.xcframework.zip"
echo "  SHA256:    $(cat "${FRAMEWORK_NAME}.xcframework.zip.sha256")"
echo ""
echo "Next: upload the zip to a GitHub release and update the Apple repo's"
echo ".core-version with the URL + sha256:<hash>."
