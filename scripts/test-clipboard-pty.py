#!/usr/bin/env python3
"""Native clipboard shortcut routing in offline demo; never reads the OS clipboard."""
import runpy
import re
import tempfile
from pathlib import Path

root = Path(__file__).resolve().parents[1]
Terminal = runpy.run_path(str(root / 'scripts/test-login-recovery-pty.py'))['Terminal']
with tempfile.TemporaryDirectory(prefix='teleaf-clipboard-keys-') as folder:
    terminal = Terminal(folder, args=('--demo',), width=110, height=40,
                        environment={'SSH_TTY': 'isolated-test'})
    try:
        terminal.wait('离线演示')
        terminal.read(.5)
        terminal.click(10, 3)
        terminal.wait('图片直接放在这条消息里')
        terminal.read(.5)
        terminal.send(b'\x1b[18~')  # F7
        # Diff output can reuse the space after SSH from the preceding frame.
        terminal.wait('无法读取本机图片剪贴板')
        terminal.send(b'\x0f')  # Ctrl+O
        terminal.wait('发送附件')
        terminal.read(.3)
        plain = re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]', b'', terminal.output)
        assert '粘贴'.encode() in plain and b'F7' in plain
        terminal.send(b'\x1b')
        terminal.read(.3)
        assert not terminal.read(.5), 'Clipboard hook caused idle redraws'
        terminal.close()
        print('PTY clipboard: PASS (F7, SSH fallback, paste button, cancel, zero idle output)')
    finally:
        terminal.cleanup()
