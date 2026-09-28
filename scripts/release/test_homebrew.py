"""Offline checks for Homebrew rendering and release asset rejection."""
import hashlib
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

from test_release import load

homebrew = load('homebrew')
packager = load('package-desktop')


class HomebrewTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        binaries, licenses = self.root / 'binaries', self.root / 'licenses'
        binaries.mkdir()
        licenses.mkdir()
        for name in packager.BINARIES:
            (binaries / name).write_text('#!/bin/sh\necho "CommitBook 1.2.3"\n')
        for name in packager.LICENSES:
            (licenses / name).write_text('Fixture license text\n')
        for target in homebrew.TARGETS.values():
            # Give each architecture a different checksum to catch swapped mappings.
            (binaries / 'commitbook').write_text(f'#!/bin/sh\n# {target}\necho "CommitBook 1.2.3"\n')
            packager.package(binaries, licenses, self.root / f'commitbook-{target}.tar.gz')
        self.write_checksums()

    def write_checksums(self):
        lines = []
        for target in homebrew.TARGETS.values():
            archive = self.root / f'commitbook-{target}.tar.gz'
            lines.append(f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n')
        (self.root / 'SHA256SUMS').write_text(''.join(lines))

    def test_formula_urls_checksums_and_installed_contents(self):
        result = homebrew.render('1.2.3', self.root)
        for target in homebrew.TARGETS.values():
            name = f'commitbook-{target}.tar.gz'
            digest = hashlib.sha256((self.root / name).read_bytes()).hexdigest()
            self.assertIn(f'url "{homebrew.BASE_URL}/1.2.3/{name}"\n      sha256 "{digest}"', result)
        for name in homebrew.BINARIES:
            self.assertIn(f'"bin/{name}"', result)
        for name in homebrew.LICENSES:
            self.assertIn(f'"share/licenses/commitbook/{name}"', result)

    def test_invalid_tags(self):
        for tag in ('v1.2.3', '01.2.3', '1.02.3', '1.2.03', '1.2.3-beta', '1.2.3+build', '../1.2.3', '1.2.3\n'):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                homebrew.render(tag, self.root)

    def test_missing_and_corrupt_archives(self):
        archive = self.root / 'commitbook-aarch64-apple-darwin.tar.gz'
        original = archive.read_bytes()
        archive.unlink()
        with self.assertRaisesRegex(ValueError, 'Missing'):
            homebrew.render('1.2.3', self.root)
        archive.write_bytes(original + b'corruption')
        with self.assertRaisesRegex(ValueError, 'Checksum mismatch'):
            homebrew.render('1.2.3', self.root)

    def test_malformed_missing_and_duplicate_checksums(self):
        path = self.root / 'SHA256SUMS'
        original = path.read_text()
        for content in ('', 'not a checksum\n', original + original.splitlines()[0] + '\n'):
            with self.subTest(content=content):
                path.write_text(content)
                with self.assertRaises(ValueError):
                    homebrew.render('1.2.3', self.root)

    def test_unsafe_or_incomplete_archive_is_rejected_even_with_valid_checksum(self):
        archive = self.root / 'commitbook-aarch64-apple-darwin.tar.gz'
        for unsafe in ('../escape', 'bin/commitbook'):
            with self.subTest(entry=unsafe):
                with tarfile.open(archive, 'w:gz') as tar:
                    entry = tarfile.TarInfo(unsafe)
                    entry.type = tarfile.SYMTYPE
                    entry.linkname = '/tmp/escape'
                    tar.addfile(entry)
                self.write_checksums()
                with self.assertRaisesRegex(ValueError, 'archive layout'):
                    homebrew.render('1.2.3', self.root)

    def test_renderer_failure_does_not_create_formula(self):
        (self.root / 'SHA256SUMS').unlink()
        output = self.root / 'rendered/commitbook.rb'
        result = subprocess.run(['python3', homebrew.__file__, '1.2.3', str(self.root), str(output)], capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(output.exists())


if __name__ == '__main__':
    unittest.main()
