#!/usr/bin/env python3
"""Exercise forced protocol / fallback byte streams; not real emulator GPU tests."""
import re
import runpy
import tempfile
from pathlib import Path
root=Path(__file__).resolve().parents[1]
Terminal=runpy.run_path(str(root/'scripts/test-login-recovery-pty.py'))['Terminal']
for protocol, env, marker in [
    ('halfblocks', {'TG_SYNC_OUTPUT':'0'}, '▄'.encode()),
    ('auto', {'TERM':'dumb'}, '▄'.encode()),
    ('auto', {}, '▄'.encode()),
    ('kitty', {'TG_CELL_SIZE':'12x24'}, b'a=T,U=1'),
    ('sixel', {}, b'\x1bP'),
    ('iterm2', {}, b'\x1b]1337;File=')
]:
    with tempfile.TemporaryDirectory(prefix='tg-terminal-smoke-') as folder:
        terminal=Terminal(folder,args=('--demo',),width=80,height=24,image_protocol=protocol,environment=env)
        try:
            terminal.wait('离线演示')
            for _ in range(40):
                terminal.read(.1)
                if '点击打开离线演示会话'.encode() in re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]',b'',terminal.output): break
            else: raise AssertionError('Demo chat list was not ready')
            terminal.send(b'\r') # Open the selected chat; narrow view shows its last media first
            for _ in range(25):
                terminal.read(.2)
                if marker in terminal.output: break
            assert marker in terminal.output, f'{protocol} did not encode its protocol'
            if env.get('TG_SYNC_OUTPUT')=='0':
                assert b'\x1b[?2026h' not in terminal.output
            if env.get('TERM')=='dumb':
                assert b'a=q' not in terminal.output, 'dumb terminal should not be queried'
            if protocol == 'auto' and not env:
                terminal.send(b'?')
                terminal.wait('使用帮助') # Query timeout must not steal input or disable raw mode
            if protocol in ('kitty','sixel','iterm2'):
                assert b'a=q' not in terminal.output, 'Forced protocol must skip capability queries'
            terminal.close()
            print(f'PTY terminal profile: PASS ({protocol}, {env})')
        finally:
            terminal.cleanup()
