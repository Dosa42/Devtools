import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parent


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


BUILD = load('build_cli', 'build-cli.py')
PUBLISH = load('publish_dist', 'publish-dist.py')


class DistributionTests(unittest.TestCase):
    def test_binary_headers_distinguish_platform_and_architecture(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable = Path(temporary) / 'fixture'
            for target in BUILD.TARGETS.values():
                header = bytearray(32)
                if target[3] == 'ELF':
                    header[:6] = b'\x7fELF\x02\x01'
                    struct.pack_into('<H', header, 18, target[4])
                else:
                    struct.pack_into('<II', header, 0, 0xFEEDFACF, target[4])
                executable.write_bytes(header)
                self.assertEqual(BUILD.executable_identity(executable, target)['format'], target[3])
                incompatible = next(value for value in BUILD.TARGETS.values() if value[3:] != target[3:])
                with self.assertRaises(RuntimeError):
                    BUILD.executable_identity(executable, incompatible)

    def test_archive_keeps_executable_permissions_and_manifest_checksums(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'fixture'
            source.mkdir()
            executable = source / 'devtool'
            executable.write_bytes(b'fixture')
            executable.chmod(0o755)
            BUILD.checksums(source)
            self.assertEqual((source / 'SHA256SUMS').read_text(), hashlib.sha256(b'fixture').hexdigest() + '  devtool\n')
            BUILD.archive(source, root / 'fixture.tar.gz', 'devtool')
            with tarfile.open(root / 'fixture.tar.gz') as archive:
                self.assertEqual(archive.getmember('devtool/devtool').mode & 0o777, 0o755)
                self.assertEqual(archive.extractfile('devtool/devtool').read(), b'fixture')

    def test_publisher_pushes_all_platforms_and_is_unchanged_on_repeat(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            checkout = root / 'checkout'
            remote = root / 'remote.git'
            incoming = root / 'artifacts'
            checkout.mkdir()
            def git(*arguments):
                return subprocess.run(['git', *arguments], cwd=checkout, check=True, capture_output=True, text=True).stdout.strip()
            git('init')
            git('config', 'user.name', 'Fixture')
            git('config', 'user.email', 'fixture@example.invalid')
            (checkout / 'all-dist').mkdir()
            (checkout / 'all-dist/README.md').write_text('fixture')
            git('add', '.')
            git('commit', '-m', 'Fixture')
            git('branch', '-M', 'main')
            git('init', '--bare', str(remote))
            git('remote', 'add', 'origin', str(remote))
            git('push', 'origin', 'main')
            previous = git('rev-parse', 'HEAD')
            revision = 'a' * 40
            for platform in PUBLISH.PLATFORMS:
                path = incoming / f'devtools-{platform}'
                path.mkdir(parents=True)
                (path / 'fixture.tar.gz').write_bytes(b'fixture')
                (path / 'manifest.json').write_text(json.dumps({'platform': platform, 'source_commit': revision, 'target': BUILD.TARGETS[platform][0], 'versions': {'devtool': '0.5.0'}, 'archive': 'fixture.tar.gz'}))
                BUILD.checksums(path)
            browser = incoming / 'wasm-ui-assets'
            browser.mkdir()
            (browser / 'devtools-wasm-ui.tar.gz').write_bytes(b'wasm-fixture')
            (browser / 'manifest.json').write_text(json.dumps({'source_commit': revision, 'archive': 'devtools-wasm-ui.tar.gz'}))
            BUILD.checksums(browser)
            with patch.object(PUBLISH, 'ROOT', checkout), patch.dict(os.environ, {'DEVTOOLS_ARTIFACTS': str(incoming), 'DEVTOOLS_SOURCE_SHA': revision}), contextlib.redirect_stdout(io.StringIO()):
                PUBLISH.main()
                published = git('rev-parse', 'HEAD')
                self.assertNotEqual(published, previous)
                self.assertEqual(git('ls-remote', 'origin', 'refs/heads/main').split()[0], published)
                PUBLISH.main()
                self.assertEqual(git('rev-parse', 'HEAD'), published)
            index = json.loads((checkout / 'all-dist/index.json').read_text())
            self.assertEqual(len(index['platforms']), 4)
            self.assertEqual(index['wasm_ui']['target'], 'wasm32-unknown-unknown')
            self.assertEqual((checkout / 'all-dist/wasm-ui/devtools-wasm-ui.tar.gz').read_bytes(), b'wasm-fixture')
            (browser / 'devtools-wasm-ui.tar.gz').write_bytes(b'changed')
            with patch.object(PUBLISH, 'ROOT', checkout), patch.dict(os.environ, {'DEVTOOLS_ARTIFACTS': str(incoming), 'DEVTOOLS_SOURCE_SHA': revision}):
                with self.assertRaisesRegex(RuntimeError, 'WASM UI checksum mismatch'):
                    PUBLISH.main()
            self.assertEqual((checkout / 'all-dist/README.md').read_text(), 'fixture')
            self.assertEqual(git('status', '--porcelain'), '')


if __name__ == '__main__':
    unittest.main()
