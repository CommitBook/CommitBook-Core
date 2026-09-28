#!/usr/bin/env python3
"""Generate Rust notices plus bundled native license texts from locked sources."""
import argparse
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
# Include the bundled native projects, not only the Rust wrapper licenses.
NATIVE = {
    'libgit2-sys': ['libgit2/COPYING', 'libgit2/deps/llhttp/LICENSE-MIT', 'libgit2/deps/zlib/LICENSE'],
    'libssh2-sys': ['libssh2/COPYING'],
    'libz-sys': ['src/zlib/LICENSE'],
    'openssl-src': ['openssl/LICENSE.txt'],
    'aws-lc-sys': ['aws-lc/LICENSE', 'aws-lc/third_party/fiat/LICENSE'],
    'ring': ['LICENSE-BoringSSL', 'LICENSE-other-bits', 'third_party/fiat/LICENSE'],
}


def generate(output, cargo_about):
    output.mkdir(parents=True, exist_ok=True)
    version = subprocess.check_output([cargo_about, '--version'], text=True).strip()
    if version != 'cargo-about 0.9.2':
        raise ValueError(f'Expected cargo-about 0.9.2, got {version}')
    notice = subprocess.check_output([
        cargo_about, 'generate', '--locked', '--workspace', '--fail',
        '--config', str(ROOT / 'scripts/release/about.toml'),
        str(ROOT / 'scripts/release/notices.hbs')], cwd=ROOT)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1'], cwd=ROOT))
    text = notice.decode()
    # Workspace crates are shipped from GitHub, not published to crates.io.
    members = set(metadata['workspace_members'])
    for package in metadata['packages']:
        if package['id'] in members:
            text = text.replace(
                f"https://crates.io/api/v1/crates/{package['name']}/{package['version']}/download",
                f"https://github.com/CommitBook/CommitBook-Core/archive/refs/tags/{package['version']}.tar.gz")
    parts = [text]
    found = set()
    for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
        if package['name'] not in NATIVE:
            continue
        found.add(package['name'])
        base = Path(package['manifest_path']).parent
        for relative in NATIVE[package['name']]:
            text = (base / relative).read_text()
            parts.append(f"\nBundled native source: {package['name']} {package['version']} / {relative}\n{text}\n")
    if found != set(NATIVE):
        raise ValueError(f'Missing bundled native license sources: {set(NATIVE) - found}')
    (output / 'THIRD_PARTY_NOTICES.txt').write_text('\n'.join(parts))
    (output / 'LICENSE').write_bytes((ROOT / 'LICENSE').read_bytes())


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('output', type=Path)
    parser.add_argument('--cargo-about', default='cargo-about')
    args = parser.parse_args()
    generate(args.output, args.cargo_about)
