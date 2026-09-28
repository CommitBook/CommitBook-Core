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
# The Swift module name comes from `module_name` in uniffi.toml and is baked
# into the generated .swift/.h/.modulemap basenames; it is not set here.

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
    cargo build --locked --release --target "$target" -p "$CRATE" --lib
}

# 1. Build all five slices.
build_target aarch64-apple-ios
build_target aarch64-apple-ios-sim
build_target x86_64-apple-ios
build_target aarch64-apple-darwin
build_target x86_64-apple-darwin

# 2. Build the host bindgen once, then generate Swift bindings from the
#    compiled library's metadata. Cross-target slice builds above deliberately
#    use --lib so they do not build an unusable bindgen executable per target.
echo ">> Building host UniFFI bindgen"
HOST_TARGET="$(rustc -vV | awk '/^host: / { print $2 }')"
[ -n "$HOST_TARGET" ] || { echo "error: could not determine Rust host target" >&2; exit 1; }
cargo build --locked --release --target "$HOST_TARGET" --bin uniffi-bindgen -p "$CRATE"

echo ">> Generating Swift bindings"
"$REPO_ROOT/target/$HOST_TARGET/release/uniffi-bindgen" \
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

# 5. Stage headers + modulemap per slice.
#
#    UniFFI derives these names from `module_name` in uniffi.toml, not from the
#    UDL namespace: module_name = "CommitBookEngine" emits CommitBookEngineFFI.h
#    / .modulemap (the C module is module_name + FFI) alongside
#    CommitBookEngine.swift. Discover them by glob so the exact prefix does not
#    matter, and fail loudly if bindgen emitted neither.
#
#    The header MUST keep its generated basename: the modulemap refers to it by
#    name (`header "CommitBookEngineFFI.h"`), so renaming it leaves the
#    modulemap pointing at a missing file and every Swift consumer fails to
#    build the module. Only the modulemap is renamed, to the `module.modulemap`
#    filename Xcode looks for in a framework's Headers directory.
stage_headers() {
    local dir="$1"
    mkdir -p "$dir"
    local hdr modmap
    hdr=$(find "$BINDINGS_DIR" -name '*FFI.h' | head -n1)
    modmap=$(find "$BINDINGS_DIR" -name '*FFI.modulemap' | head -n1)
    [ -n "$hdr" ] || { echo "error: no generated *FFI.h in $BINDINGS_DIR" >&2; exit 1; }
    [ -n "$modmap" ] || { echo "error: no generated *FFI.modulemap in $BINDINGS_DIR" >&2; exit 1; }
    cp "$hdr" "$dir/$(basename "$hdr")"
    cp "$modmap" "$dir/module.modulemap"

    # Self-check: every header the modulemap references must exist alongside it.
    # A mismatch still zips "successfully" but breaks every Swift consumer, so
    # fail the build here instead of shipping an unimportable framework.
    local referenced
    while IFS= read -r referenced; do
        [ -n "$referenced" ] || continue
        [ -f "$dir/$referenced" ] || {
            echo "error: module.modulemap references '$referenced' but it is not staged in $dir" >&2
            exit 1
        }
    done < <(awk -F'"' '/[[:space:]]header[[:space:]]/{print $2}' "$dir/module.modulemap")
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

# License texts travel with the framework, including statically linked C libraries.
python3 scripts/release/notices.py "$OUT_DIR/${FRAMEWORK_NAME}.xcframework/Licenses"

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
