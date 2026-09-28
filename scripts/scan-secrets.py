#!/usr/bin/env python3
"""Run a checksum-pinned Gitleaks against all available Git refs, fully redacted."""
import argparse
import hashlib
import io
import platform
from pathlib import Path
import subprocess
import tarfile
import urllib.request

VERSION = '8.30.1'
DIGESTS = {
    ('Darwin', 'arm64'): ('darwin_arm64', 'b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5'),
    ('Darwin', 'x86_64'): ('darwin_x64', 'dfe101a4db2255fc85120ac7f3d25e4342c3c20cf749f2c20a18081af1952709'),
    ('Linux', 'x86_64'): ('linux_x64', '551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb'),
    ('Linux', 'aarch64'): ('linux_arm64', 'e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080'),
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--report', type=Path, default=Path('target/secrets-history.json'))
    args = parser.parse_args()
    target, expected = DIGESTS[(platform.system(), platform.machine())]
    cache = Path('target/security-tools')
    cache.mkdir(parents=True, exist_ok=True)
    archive = cache / f'gitleaks_{VERSION}_{target}.tar.gz'
    if not archive.exists():
        url = f'https://github.com/gitleaks/gitleaks/releases/download/v{VERSION}/{archive.name}'
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read()
        if hashlib.sha256(data).hexdigest() != expected:
            raise ValueError('Gitleaks download checksum mismatch')
        archive.write_bytes(data)
    data = archive.read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError('Cached Gitleaks archive checksum mismatch')
    binary = cache / f'gitleaks-{VERSION}-{target}'
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as tar:
        member = tar.getmember('gitleaks')
        if not member.isfile():
            raise ValueError('Gitleaks archive contains an unexpected binary type')
        binary.write_bytes(tar.extractfile(member).read())
    binary.chmod(0o755)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    result = subprocess.run([str(binary.resolve()), 'git', '--no-banner', '--redact=100',
        '--log-opts=--all', '--report-format=json', f'--report-path={args.report}'])
    raise SystemExit(result.returncode)


if __name__ == '__main__':
    main()
