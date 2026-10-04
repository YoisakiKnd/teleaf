#!/usr/bin/env python3
"""Verify bundled application credentials without submitting a phone or code.
First build with the synthetic test pair: TELEAF_APP_API_ID=1 and
TELEAF_APP_API_HASH=00000000000000000000000000000000. Rebuild without them after.
Requires macOS/Linux and TDLib. Never reads existing account data.
"""
import json
import argparse
import re
import tempfile
from pathlib import Path
from importlib.machinery import SourceFileLoader

Terminal = SourceFileLoader(
    'recovery', str(Path(__file__).with_name('test-login-recovery-pty.py'))
).load_module().Terminal

parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, help='Use a packaged executable')
parser.add_argument('--release', action='store_true', help='Validate a real bundled pair without printing it')
options = parser.parse_args()

for existing in [False, True]:
    with tempfile.TemporaryDirectory(prefix='teleaf-bundled-api-') as folder:
        path = Path(folder)
        if existing:
            (path / 'config.json').write_text(json.dumps({
                'api_id': 2, 'api_hash': '1' * 32, 'database_key': 'isolated-test-key',
            }))
        terminal = Terminal(path, executable=options.binary)
        try:
            terminal.wait('手机号')
            assert '首次设置'.encode() not in terminal.output
            saved = json.loads((path / 'config.json').read_text())
            if options.release and not existing:
                assert isinstance(saved['api_id'], int) and saved['api_id'] > 0
                assert re.fullmatch(r'[a-fA-F0-9]{32}', saved['api_hash'])
            else:
                assert saved['api_id'] == (2 if existing else 1)
                assert saved['api_hash'] == ('1' if existing else '0') * 32
            assert saved['database_key']
            terminal.close()
        finally:
            terminal.cleanup()

print('PTY bundled API: PASS (fresh install skips setup, existing credentials preserved, no phone/code submitted)')
