#!/usr/bin/env bash
# This script needs bash. Piped into `sh` (dash on Debian and Ubuntu) it
# would stop at the first bash-only line with a syntax error, so explain
# instead. Kept POSIX so every shell can run it.
if [ -z "${BASH_VERSION:-}" ]; then
    echo "This installer needs bash. Run it with:" >&2
    echo "  curl -fsSL https://raw.githubusercontent.com/CommitBook/CommitBook-Core/main/scripts/release/install.sh | bash" >&2
    exit 1
fi
set -euo pipefail

REPO="CommitBook/CommitBook-Core"
INSTALL_PREFIX="${COMMITBOOK_INSTALL_PREFIX:-${HOME}/.local}"
INSTALL_DIR="${INSTALL_PREFIX}/bin"

# Verify a downloaded file against a SHA256SUMS manifest.
# The manifest lists every release target, so we filter it down to the one
# file we actually downloaded before handing it to the checksum tool.
verify_checksum() {
    local dir="$1" file="$2" sums="$3"
    local expected
    local -a sums_cmd

    expected="$(awk -v want="$file" '{ name = $2; sub(/^\*/, "", name); if (name == want) { print; exit } }' "$sums")"
    if [ -z "$expected" ]; then
        echo "No checksum for ${file} in SHA256SUMS" >&2
        exit 1
    fi

    printf '%s\n' "$expected" > "${dir}/SHA256SUMS.filtered"

    if command -v sha256sum >/dev/null 2>&1; then
        sums_cmd=(sha256sum -c SHA256SUMS.filtered)
    elif command -v shasum >/dev/null 2>&1; then
        sums_cmd=(shasum -a 256 -c SHA256SUMS.filtered)
    else
        echo "Neither sha256sum nor shasum found; cannot verify download" >&2
        exit 1
    fi

    if ! (cd "$dir" && "${sums_cmd[@]}" >/dev/null 2>&1); then
        echo "Checksum mismatch for ${file}" >&2
        echo "Expected: ${expected}" >&2
        echo "The download may be corrupt or tampered with. Aborting." >&2
        exit 1
    fi

    echo "Checksum verified: ${file}"
}

# Also used by fixture tests; no networking or shell-profile changes here.
install_archive() {
    local archive="$1" staging="$2" name
    # Reject extra paths before extraction; published archives contain only files.
    local listing
    listing="$(tar tzf "$archive")"
    while IFS= read -r name; do
        case "$name" in
            bin/commitbook|bin/cobo|bin/commitbook-tui|bin/commitbook-web|share/licenses/commitbook/LICENSE|share/licenses/commitbook/THIRD_PARTY_NOTICES.txt) ;;
            *) echo "Unexpected archive entry: $name" >&2; return 1 ;;
        esac
    done <<< "$listing"
    if ! tar tvzf "$archive" | awk 'substr($0, 1, 1) != "-" { exit 1 }'; then
        echo "Archive must contain regular files only" >&2
        return 1
    fi
    mkdir -p "$staging"
    tar xzf "$archive" -C "$staging"
    for name in commitbook cobo commitbook-tui commitbook-web; do
        [ -f "$staging/bin/$name" ] && [ ! -L "$staging/bin/$name" ] || return 1
    done
    for name in LICENSE THIRD_PARTY_NOTICES.txt; do
        [ -f "$staging/share/licenses/commitbook/$name" ] && [ ! -L "$staging/share/licenses/commitbook/$name" ] || return 1
    done
    mkdir -p "$INSTALL_DIR" "$INSTALL_PREFIX/share/licenses/commitbook"
    for name in commitbook cobo commitbook-tui commitbook-web; do
        install -m 755 "$staging/bin/$name" "$INSTALL_DIR/$name"
    done
    for name in LICENSE THIRD_PARTY_NOTICES.txt; do
        install -m 644 "$staging/share/licenses/commitbook/$name" "$INSTALL_PREFIX/share/licenses/commitbook/$name"
    done
}

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

    local base="https://github.com/${REPO}/releases/download/${tag}"
    local archive="commitbook-${target}.tar.gz"
    local tmp
    tmp="$(mktemp -d)"
    COMMITBOOK_INSTALL_TMP="$tmp"
    trap 'rm -rf -- "$COMMITBOOK_INSTALL_TMP"' EXIT

    echo "Downloading ${base}/${archive}..."
    curl -fsSL "${base}/${archive}" -o "${tmp}/${archive}"

    echo "Downloading ${base}/SHA256SUMS..."
    curl -fsSL "${base}/SHA256SUMS" -o "${tmp}/SHA256SUMS"

    verify_checksum "$tmp" "$archive" "${tmp}/SHA256SUMS"

    echo "Extracting to ${INSTALL_DIR}..."
    install_archive "${tmp}/${archive}" "${tmp}/unpacked"

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

if [[ -z "${BASH_SOURCE[0]:-}" || "${BASH_SOURCE[0]:-}" == "$0" ]]; then
    main "$@"
fi
