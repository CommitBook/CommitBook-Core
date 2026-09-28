#!/usr/bin/env python3
"""Validate version and commit before building or publishing release assets."""
import argparse
import re
import subprocess
import tomllib
from pathlib import Path


def validate(tag, sha, fetch=False):
    version = tomllib.loads(Path('Cargo.toml').read_text())['workspace']['package']['version']
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', tag) or tag != version:
        raise ValueError(f'Release tag {tag!r} must equal workspace version {version!r}')
    if fetch:
        subprocess.run(['git', 'fetch', '--no-tags', '--depth=1', 'origin',
                        f'refs/tags/{tag}:refs/tags/{tag}'], check=True)
    tagged = subprocess.check_output(['git', 'rev-parse', '--verify', f'refs/tags/{tag}^{{commit}}'], text=True).strip()
    built = subprocess.check_output(['git', 'rev-parse', '--verify', f'{sha}^{{commit}}'], text=True).strip()
    if tagged != built:
        raise ValueError(f'Tag points at {tagged}, but the built commit is {built}')
    print(f'Validated release {tag} at {built}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('tag')
    parser.add_argument('--sha', default='HEAD')
    parser.add_argument('--fetch', action='store_true')
    args = parser.parse_args()
    validate(args.tag, args.sha, args.fetch)
