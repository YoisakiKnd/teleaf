#!/usr/bin/env python3
"""Offline packaging tests: verified extraction, platform manifests and archive layout."""
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from tdlib_runtime import checksum, extract

ROOT = Path(__file__).resolve().parents[1]


def module(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), ROOT / 'scripts' / f'{name}.py')
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


packages = module('generate-packages')
release = module('package-release')


class PackagingTests(unittest.TestCase):
    def test_installer_reports_errors_with_legacy_console_encoding(self):
        with tempfile.TemporaryDirectory() as folder:
            archive = Path(folder) / 'bad.zip'
            archive.write_bytes(b'not a verified runtime')
            environment = dict(os.environ, PYTHONIOENCODING='cp1252', PYTHONUTF8='0')
            result = subprocess.run(
                [sys.executable, str(ROOT / 'scripts/install-tdlib.py'),
                 '--platform', 'windows-x86_64', '--archive', str(archive),
                 '--dest', str(Path(folder) / 'runtime')],
                env=environment, capture_output=True,
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn('TDLib 安装失败', result.stderr.decode('utf-8'))
            self.assertNotIn(b'UnicodeEncodeError', result.stderr)
            self.assertFalse((Path(folder) / 'runtime').exists())

    def test_download_handles_legacy_console_encoding(self):
        # Reproduce the Windows CI pipe encoding with an offline download fixture.
        code = '''
import hashlib, io, sys, tempfile, zipfile
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0, str(Path(sys.argv[1]).parent))
import tdlib_runtime
data = io.BytesIO()
with zipfile.ZipFile(data, 'w') as z:
    z.writestr('tdlib/bin/tdjson.dll', b'runtime')
payload = data.getvalue()
tdlib_runtime.METADATA['sha256']['windows-x86_64'] = hashlib.sha256(payload).hexdigest()
tdlib_runtime.configure_console()
with tempfile.TemporaryDirectory() as folder:
    with patch('urllib.request.urlopen', return_value=io.BytesIO(payload)):
        result = tdlib_runtime.install(Path(folder) / 'runtime', 'windows-x86_64', cache=Path(folder) / 'cache')
        assert result.read_bytes() == b'runtime'
'''
        result = subprocess.run(
            [sys.executable, '-c', code, str(ROOT / 'scripts/package-release.py')],
            env=dict(os.environ, PYTHONIOENCODING='cp1252', PYTHONUTF8='0'), capture_output=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr.decode('utf-8', errors='replace'))
        self.assertIn('下载 TDLib', result.stdout.decode('utf-8'))

    def test_bad_checksum_never_replaces_existing_runtime(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            archive = folder / 'runtime.zip'
            with zipfile.ZipFile(archive, 'w') as z:
                z.writestr('tdlib/lib/libtdjson.dylib', b'new')
            destination = folder / 'installed'
            destination.mkdir()
            (destination / 'libtdjson.dylib').write_bytes(b'old')
            with self.assertRaises(ValueError):
                extract(archive, destination, 'macos-aarch64', '0' * 64)
            self.assertEqual((destination / 'libtdjson.dylib').read_bytes(), b'old')

    def test_extracts_only_runtimes_and_keeps_windows_dependencies(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            archive = folder / 'runtime.zip'
            with zipfile.ZipFile(archive, 'w') as z:
                for name in ('tdjson.dll', 'zlib1.dll', 'libcrypto-3-x64.dll', 'libssl-3-x64.dll'):
                    z.writestr('tdlib/bin/' + name, b'runtime')
                z.writestr('tdlib/bin/../../outside.dll', b'bad')
                z.writestr('tdlib/bin/C:\\outside.dll', b'bad')
                z.writestr('tdlib/lib/large-static.lib', b'ignore')
            target = folder / 'installed'
            extract(archive, target, 'windows-x86_64', checksum(archive))
            self.assertEqual({p.name for p in target.iterdir()}, {'tdjson.dll', 'zlib1.dll', 'libcrypto-3-x64.dll', 'libssl-3-x64.dll'})
            self.assertFalse((folder / 'outside.dll').exists())

    def test_missing_primary_does_not_install_partial_files(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            archive = folder / 'runtime.zip'
            with zipfile.ZipFile(archive, 'w') as z:
                z.writestr('tdlib/bin/zlib1.dll', b'runtime')
            target = folder / 'installed'
            with self.assertRaises(ValueError):
                extract(archive, target, 'windows-x86_64', checksum(archive))
            self.assertFalse(target.exists())

    def test_generated_urls_hashes_and_homebrew_syntax(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            for platform in packages.PLATFORMS:
                extension = 'zip' if platform.startswith('windows-') else 'tar.gz'
                (folder / f'teleaf-0.1.0-{platform}.{extension}').write_bytes(platform.encode())
            formula, manifest = packages.generate('example/teleaf', '0.1.0', folder, folder / 'generated')
            value = json.loads(manifest.read_text())
            self.assertEqual(value['bin'], 'teleaf.exe')
            self.assertEqual(value['license'], 'MIT')
            self.assertEqual(value['architecture']['64bit']['hash'], checksum(folder / 'teleaf-0.1.0-windows-x86_64.zip'))
            self.assertIn('example/teleaf/releases/download/v0.1.0/', value['architecture']['64bit']['url'])
            self.assertEqual(len((folder / 'SHA256SUMS').read_text().splitlines()), 5)
            self.assertIn('libexec.install', formula.read_text())
            self.assertEqual(formula.read_text().count('sha256 "'), 4)
            if shutil.which('ruby'):
                subprocess.run(['ruby', '-c', str(formula)], check=True, stdout=subprocess.DEVNULL)
            with self.assertRaises(ValueError):
                packages.generate('bad"/repo', '0.1.0', folder, folder / 'bad')
            with self.assertRaises(FileNotFoundError):
                packages.generate('example/teleaf', '9.9.9', folder, folder / 'missing')

    def test_archive_keeps_runtime_next_to_executable(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            staging = folder / 'stage'
            (staging / 'tdlib').mkdir(parents=True)
            (staging / 'teleaf.exe').write_bytes(b'executable')
            (staging / 'tdlib/tdjson.dll').write_bytes(b'runtime')
            archive = folder / 'teleaf.zip'
            release.write_archive(staging, archive)
            with zipfile.ZipFile(archive) as z:
                self.assertEqual(set(z.namelist()), {'teleaf.exe', 'tdlib/tdjson.dll'})
                self.assertEqual(z.read('tdlib/tdjson.dll'), b'runtime')


if __name__ == '__main__':
    unittest.main()
