#!/usr/bin/env python3
"""Verify release archives and render the binary-only CommitBook tap formula."""
import argparse
import hashlib
from pathlib import Path
import re
from string import Template
import tarfile

TARGETS = {
    'macos_arm': 'aarch64-apple-darwin',
    'macos_intel': 'x86_64-apple-darwin',
    'linux_arm': 'aarch64-unknown-linux-gnu',
    'linux_intel': 'x86_64-unknown-linux-gnu',
}
BINARIES = ('commitbook', 'cobo', 'commitbook-tui', 'commitbook-web')
LICENSES = ('LICENSE', 'THIRD_PARTY_NOTICES.txt')
BASE_URL = 'https://github.com/CommitBook/CommitBook-Core/releases/download'


def verify_assets(assets):
    """Check every supported platform before producing a publishable formula."""
    checksums = {}
    for line in (assets / 'SHA256SUMS').read_text().splitlines():
        match = re.fullmatch(r'([0-9a-f]{64})  (commitbook-[a-z0-9_-]+\.tar\.gz)', line)
        if not match or match[2] in checksums:
            raise ValueError('Malformed or duplicate SHA256SUMS entry')
        checksums[match[2]] = match[1]
    names = {f'commitbook-{target}.tar.gz' for target in TARGETS.values()}
    if checksums.keys() != names:
        raise ValueError('SHA256SUMS must contain exactly the four desktop archives')
    expected = {f'bin/{name}' for name in BINARIES}
    expected |= {f'share/licenses/commitbook/{name}' for name in LICENSES}
    result = {}
    for key, target in TARGETS.items():
        name = f'commitbook-{target}.tar.gz'
        archive = assets / name
        if not archive.is_file() or archive.is_symlink():
            raise ValueError(f'Missing or unsafe release archive: {name}')
        with archive.open('rb') as stream:
            actual = hashlib.file_digest(stream, 'sha256').hexdigest()
        if actual != checksums[name]:
            raise ValueError(f'Checksum mismatch: {name}')
        # Inspect without extracting or executing downloaded content.
        with tarfile.open(archive, 'r:gz') as tar:
            members = tar.getmembers()
            if len(members) != len(expected) or {m.name for m in members} != expected:
                raise ValueError(f'Unexpected archive layout: {name}')
            for member in members:
                if not member.isfile() or member.size == 0:
                    raise ValueError(f'Unsafe or empty archive entry: {member.name}')
                if member.name.startswith('bin/') and member.mode & 0o111 != 0o111:
                    raise ValueError(f'Non-executable binary: {member.name}')
        result[key] = actual
    return result


def render(tag, assets):
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', tag):
        raise ValueError('Expected a bare version such as 1.2.3 (no v prefix)')
    hashes = verify_assets(assets)
    values = {}
    for key, target in TARGETS.items():
        values[f'{key}_url'] = f'{BASE_URL}/{tag}/commitbook-{target}.tar.gz'
        values[f'{key}_sha'] = hashes[key]
    return Template(Path(__file__).with_name('commitbook.rb.tmpl').read_text()).substitute(values)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('tag')
    parser.add_argument('assets', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    formula = render(args.tag, args.assets)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(formula)
