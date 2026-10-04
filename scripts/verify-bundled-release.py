#!/usr/bin/env python3
"""Gate publication on actual bundled credentials; runs the Linux x64 archive.
Only initializes temporary data and reaches the phone prompt. No account login.
"""
import argparse
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--version', required=True)
parser.add_argument('--assets', type=Path, default=ROOT / 'target/dist')
options = parser.parse_args()
archive = options.assets / f'teleaf-{options.version}-linux-x86_64.tar.gz'
with tempfile.TemporaryDirectory(prefix='teleaf-release-gate-') as directory:
    with tarfile.open(archive) as package:
        package.extractall(directory, filter='data')
    binary = Path(directory) / 'teleaf'
    result = subprocess.run([str(binary), '--version'], capture_output=True, text=True, check=True)
    assert result.stdout.strip() == f'Teleaf {options.version}'
    subprocess.run([str(binary), '--check'], check=True)
    subprocess.run([sys.executable, str(ROOT / 'scripts/test-bundled-api-pty.py'),
                    '--release', '--binary', str(binary)], check=True)
print('Actual release archive: bundled login startup and existing config PASS')
