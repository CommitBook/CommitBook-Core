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
            for tag in ['0.11.0', 'v0.10.0', '../bad', '0.10.0-beta', '0.10.0+build', '0.10.0\n']:
                self.assertNotEqual(subprocess.run(command + [tag], cwd=root, capture_output=True).returncode, 0)
            (root / 'other').write_text('second commit')
            git('add', '.')
            git('commit', '-m', 'second')
            self.assertNotEqual(subprocess.run(command + ['0.10.0'], cwd=root, capture_output=True).returncode, 0)

    def test_leading_zero_tags_fail_even_when_manifest_matches(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for tag in ('00.10.0', '0.010.0', '0.10.00'):
                with self.subTest(tag=tag):
                    (root / 'Cargo.toml').write_text(f'[workspace.package]\nversion = "{tag}"\n')
                    result = subprocess.run(
                        ['python3', str(ROOT / 'scripts/release/validate-release.py'), tag],
                        cwd=root, capture_output=True, text=True)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('must equal workspace version', result.stderr)

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
            command = ['bash', '-c', 'source "$1"; verify_checksum "$2" fixture.tar.gz "$2/SHA256SUMS"; install_archive "$2/fixture.tar.gz" "$2/unpacked" 1.2.0', 'fixture', str(ROOT / 'scripts/release/install.sh'), str(root)]
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue((root / 'prefix/bin/cobo').is_file())
            self.assertTrue((root / 'prefix/bin/cbook').is_file())
            self.assertTrue((root / 'prefix/share/licenses/commitbook/LICENSE').is_file())
            archive.write_bytes(archive.read_bytes() + b'corruption')
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Checksum mismatch', result.stderr)
            for name in ('cobo', 'cbook'):
                with self.subTest(missing_binary=name):
                    binary = binaries / name
                    contents = binary.read_bytes()
                    binary.unlink()
                    with self.assertRaises(ValueError):
                        packager.package(binaries, licenses, root / 'incomplete.tar.gz')
                    binary.write_bytes(contents)

    def test_installer_rejects_archive_missing_cbook(self):
        packager = load('package-desktop')
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = root / 'incomplete.tar.gz'
            with tarfile.open(archive, 'w:gz') as tar:
                for name in packager.BINARIES:
                    if name != 'cbook':
                        binary = root / name
                        binary.write_text('#!/bin/sh\necho fixture\n')
                        tar.add(binary, arcname=f'bin/{name}')
                for name in packager.LICENSES:
                    license_file = root / name
                    license_file.write_text('fixture license text\n')
                    tar.add(license_file, arcname=f'share/licenses/commitbook/{name}')
            env = dict(os.environ, COMMITBOOK_INSTALL_PREFIX=str(root / 'prefix'))
            command = ['bash', '-c', 'source "$1"; install_archive "$2/incomplete.tar.gz" "$2/unpacked" "$3"', 'fixture', str(ROOT / 'scripts/release/install.sh'), str(root)]
            for tag in ('1.2.0', '1.10.0', '2.0.0', 'v1.2.0', '1.02.0'):
                with self.subTest(tag=tag):
                    result = subprocess.run(command + [tag], env=env, capture_output=True, text=True)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse((root / 'prefix/bin').exists())

    def test_piped_installer_runs_with_fixture_downloads(self):
        for tag, include_cbook in (('1.1.0', False), ('1.2.0', True), ('1.10.0', True), ('2.0.0', True)):
            with self.subTest(tag=tag):
                self.check_piped_installer(tag, include_cbook)

    def check_piped_installer(self, tag, include_cbook):
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
            if include_cbook:
                packager.package(binaries, licenses, archive)
            else:
                with tarfile.open(archive, 'w:gz') as tar:
                    for name in packager.BINARIES:
                        if name != 'cbook':
                            tar.add(binaries / name, arcname=f'bin/{name}')
                    for name in packager.LICENSES:
                        tar.add(licenses / name, arcname=f'share/licenses/commitbook/{name}')
            (root / 'SHA256SUMS').write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + '  ' + archive.name + '\n')
            mocks = root / 'mocks'; mocks.mkdir()
            (mocks / 'uname').write_text('#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo x86_64;; esac\n')
            (mocks / 'curl').write_text("""#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
args = sys.argv[1:]
url = next(arg for arg in args if arg.startswith('https://'))
if url.endswith('/latest'):
    print(json.dumps({'tag_name': os.environ['FIXTURE_RELEASE_TAG']}))
else:
    assert f"/download/{os.environ['FIXTURE_RELEASE_TAG']}/" in url
    source = pathlib.Path(os.environ['FIXTURE_DOWNLOAD_ROOT']) / url.rsplit('/', 1)[1]
    shutil.copyfile(source, args[args.index('-o') + 1])
""")
            for script in mocks.iterdir(): script.chmod(0o755)
            env = dict(os.environ, PATH=str(mocks) + os.pathsep + os.environ['PATH'],
                       FIXTURE_DOWNLOAD_ROOT=str(root), FIXTURE_RELEASE_TAG=tag,
                       COMMITBOOK_INSTALL_PREFIX=str(root / 'installed'))
            result = subprocess.run(['bash'], input=(ROOT / 'scripts/release/install.sh').read_text(),
                                    env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            for name in packager.BINARIES:
                self.assertEqual((root / f'installed/bin/{name}').exists(), include_cbook if name == 'cbook' else True)
            for name in packager.LICENSES:
                self.assertTrue((root / f'installed/share/licenses/commitbook/{name}').exists())


if __name__ == '__main__':
    unittest.main()
