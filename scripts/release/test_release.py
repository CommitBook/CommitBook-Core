"""Offline end-to-end checks for release validation, archives and installation."""
import hashlib
import importlib.util
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ReleaseTests(unittest.TestCase):
    def test_tag_must_match_version_and_exact_commit(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            def git(*args):
                return subprocess.check_output(['git', '-c', 'user.name=Test', '-c', 'user.email=test@example.com', '-c', 'commit.gpgsign=false', *args], cwd=root, stderr=subprocess.DEVNULL)
            git('init')
            (root / 'Cargo.toml').write_text('[workspace.package]\nversion = "0.10.0"\n')
            git('add', '.')
            git('commit', '-m', 'fixture')
            git('tag', '0.10.0')
            command = ['python3', str(ROOT / 'scripts/release/validate-release.py')]
            self.assertEqual(subprocess.run(command + ['0.10.0'], cwd=root, capture_output=True).returncode, 0)
            for tag in ['0.11.0', 'v0.10.0', '../bad']:
                self.assertNotEqual(subprocess.run(command + [tag], cwd=root, capture_output=True).returncode, 0)
            (root / 'other').write_text('second commit')
            git('add', '.')
            git('commit', '-m', 'second')
            self.assertNotEqual(subprocess.run(command + ['0.10.0'], cwd=root, capture_output=True).returncode, 0)

    def test_archive_installation_and_checksum_rejection(self):
        packager = load('package-desktop')
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binaries, licenses = root / 'binaries', root / 'licenses'
            binaries.mkdir(); licenses.mkdir()
            for name in packager.BINARIES:
                (binaries / name).write_text('#!/bin/sh\necho fixture\n')
            for name in packager.LICENSES:
                (licenses / name).write_text('fixture license text\n')
            archive = root / 'fixture.tar.gz'
            packager.package(binaries, licenses, archive)
            with tarfile.open(archive) as tar:
                self.assertEqual(set(tar.getnames()), {f'bin/{n}' for n in packager.BINARIES} | {f'share/licenses/commitbook/{n}' for n in packager.LICENSES})
                self.assertTrue(all(member.isfile() for member in tar))
            sums = root / 'SHA256SUMS'
            sums.write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + '  fixture.tar.gz\n')
            env = dict(os.environ, COMMITBOOK_INSTALL_PREFIX=str(root / 'prefix'))
            command = ['bash', '-c', 'source "$1"; verify_checksum "$2" fixture.tar.gz "$2/SHA256SUMS"; install_archive "$2/fixture.tar.gz" "$2/unpacked"', 'fixture', str(ROOT / 'install.sh'), str(root)]
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue((root / 'prefix/bin/cobo').is_file())
            self.assertTrue((root / 'prefix/share/licenses/commitbook/LICENSE').is_file())
            archive.write_bytes(archive.read_bytes() + b'corruption')
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Checksum mismatch', result.stderr)
            (binaries / 'cobo').unlink()
            with self.assertRaises(ValueError):
                packager.package(binaries, licenses, root / 'incomplete.tar.gz')

    def test_piped_installer_runs_with_fixture_downloads(self):
        packager = load('package-desktop')
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binaries, licenses = root / 'binaries', root / 'licenses'
            binaries.mkdir(); licenses.mkdir()
            for name in packager.BINARIES:
                (binaries / name).write_text('#!/bin/sh\necho fixture\n')
            for name in packager.LICENSES:
                (licenses / name).write_text('license fixture')
            archive = root / 'commitbook-x86_64-unknown-linux-gnu.tar.gz'
            packager.package(binaries, licenses, archive)
            (root / 'SHA256SUMS').write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + '  ' + archive.name + '\n')
            mocks = root / 'mocks'; mocks.mkdir()
            (mocks / 'uname').write_text('#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo x86_64;; esac\n')
            (mocks / 'curl').write_text("""#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
args = sys.argv[1:]
url = next(arg for arg in args if arg.startswith('https://'))
if url.endswith('/latest'):
    print(json.dumps({'tag_name': '0.10.0'}))
else:
    source = pathlib.Path(os.environ['FIXTURE_DOWNLOAD_ROOT']) / url.rsplit('/', 1)[1]
    shutil.copyfile(source, args[args.index('-o') + 1])
""")
            for script in mocks.iterdir(): script.chmod(0o755)
            env = dict(os.environ, PATH=str(mocks) + os.pathsep + os.environ['PATH'],
                       FIXTURE_DOWNLOAD_ROOT=str(root), COMMITBOOK_INSTALL_PREFIX=str(root / 'installed'))
            result = subprocess.run(['bash'], input=(ROOT / 'install.sh').read_text(),
                                    env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue((root / 'installed/bin/cobo').exists())
            self.assertTrue((root / 'installed/share/licenses/commitbook/LICENSE').exists())


if __name__ == '__main__':
    unittest.main()
