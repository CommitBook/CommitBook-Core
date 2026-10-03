#!/usr/bin/env python3
"""Build the only supported desktop archive layout; refuse incomplete inputs."""
import argparse
import tarfile
from pathlib import Path

BINARIES = ('commitbook', 'cobo', 'cbook', 'commitbook-tui', 'commitbook-web')
LICENSES = ('LICENSE', 'THIRD_PARTY_NOTICES.txt')


def package(binary_dir, licenses, output):
    entries = [(binary_dir / name, f'bin/{name}') for name in BINARIES]
    entries += [(licenses / name, f'share/licenses/commitbook/{name}') for name in LICENSES]
    for source, _ in entries:
        if not source.is_file() or source.is_symlink() or source.stat().st_size == 0:
            raise ValueError(f'Missing or unsafe artifact input: {source}')
    with tarfile.open(output, 'w:gz') as archive:
        for source, destination in entries:
            info = archive.gettarinfo(str(source), destination)
            info.uid = info.gid = 0
            info.uname = info.gname = ''
            info.mode = 0o755 if destination.startswith('bin/') else 0o644
            with source.open('rb') as stream:
                archive.addfile(info, stream)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('binary_dir', type=Path)
    parser.add_argument('licenses', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    package(args.binary_dir, args.licenses, args.output)
