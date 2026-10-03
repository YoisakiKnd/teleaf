#!/usr/bin/env python3
"""Offline quick-save/repeat through native terminal input; never sends to Telegram."""
import runpy
import tempfile
from pathlib import Path

root = Path(__file__).resolve().parents[1]
Terminal = runpy.run_path(str(root / 'scripts/test-login-recovery-pty.py'))['Terminal']

for width, height in [(110, 40), (80, 24)]:
    with tempfile.TemporaryDirectory(prefix='tg-quick-messages-') as folder:
        terminal = Terminal(folder, args=('--demo',), width=width, height=height)
        try:
            terminal.wait('离线演示')
            terminal.read(.4)
            terminal.click(10, 3)
            terminal.wait('点击输入框，再点击发送。')
            terminal.read(.4)
            terminal.send(b'S')
            terminal.wait('已收藏到收藏夹')
            terminal.send(b'D')
            terminal.wait('已复读到当前会话')
            terminal.send(b'\r')
            terminal.wait('消息操作')
            terminal.send(b'S')
            terminal.wait('已收藏到收藏夹')  # Cached Saved Messages chat ID
            terminal.click(width - 25, height - 4)
            terminal.paste('draft-survives-SD')
            terminal.send(b'\t\r')  # Browse selected message with an unsent draft
            terminal.wait('消息操作')
            terminal.send(b'D')
            terminal.wait('已复读到当前会话')
            terminal.send(b'\x1b')
            terminal.read(.2)
            terminal.click(width - 25, height - 4)
            terminal.send(b'\r')
            assert b'draft-survives-SD' in terminal.read(.6), 'Quick action lost composer draft'
            assert not (Path(folder) / 'config.json').exists(), 'Demo touched account configuration'
            terminal.close()
            print(f'PTY quick messages {width}x{height}: PASS (save, repeat, cached save, message menu, draft preservation, isolated demo)')
        finally:
            terminal.cleanup()
