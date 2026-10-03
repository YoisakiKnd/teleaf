#!/usr/bin/env python3
"""Offline PTY regression: attachments and Telegram-like sticker tray."""
import re
import runpy
import tempfile
from pathlib import Path
root = Path(__file__).resolve().parents[1]
Terminal = runpy.run_path(str(root/'scripts/test-login-recovery-pty.py'))['Terminal']
def plain(data):
    return re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]', b'', data)
with tempfile.TemporaryDirectory(prefix='tg-attachments-smoke-') as folder:
    a = Path(folder)/'hello 文件.txt'
    b = Path(folder)/'second file.txt'
    a.write_text('hello')
    b.write_text('world')
    terminal = Terminal(folder, args=('--demo',), width=110, height=40)
    try:
        terminal.wait('离线演示')
        for _ in range(40):
            terminal.read(.1)
            if '点击打开离线演示会话'.encode() in plain(terminal.output): break
        else: raise AssertionError('Demo chat list was not ready')
        terminal.click(10,3)
        terminal.wait('图片直接放在这条消息里')
        terminal.read(.5)
        terminal.click(50,36)
        terminal.paste('draft-to-preserve')
        terminal.send(b'\x0f') # Ctrl+O while composing
        terminal.wait('发送附件')
        terminal.paste(f'"{a}" "{b}"')
        assert b'secondfile.txt' in plain(terminal.output).replace(b' ',b''), 'Multiple dragged paths not staged'
        terminal.send(b'\t\t') # Browser -> Path -> Caption
        terminal.read(.2)
        terminal.paste('attachment-caption')
        terminal.send(b'\x1b[19~') # F8 works without modified Enter support
        terminal.wait('演示附件')
        terminal.read(.5)
        assert b'attachment-caption' in plain(terminal.output), 'Attachment caption was lost'
        terminal.send(b'\x0f')
        terminal.wait('发送附件')
        terminal.send(b'\x1b')
        terminal.read(.3)
        terminal.send(b'\tt') # Browse messages then open sticker tray
        terminal.wait('贴纸 · 点击发送')
        terminal.read(.5)
        terminal.click(48,26) # First thumbnail above the composer
        terminal.wait('贴纸已提交')
        terminal.read(.3)
        terminal.click(68,22) # Installed pack tab
        terminal.wait('演示贴纸包')
        terminal.read(.3)
        terminal.send(b'/')
        terminal.read(.2)
        terminal.paste('猫')
        terminal.send(b'\r')
        terminal.wait('搜索：猫')
        terminal.read(.6)
        before = len(terminal.output)
        terminal.read(.5)
        assert len(terminal.output) == before, 'Sticker tray kept repainting at idle'
        terminal.send(b'\x1b')
        terminal.read(.3)
        assert not (Path(folder)/'config.json').exists()
        terminal.close()
        print('PTY attachments/stickers: PASS (Ctrl+O, quoted multi-path paste, caption, F8 grouped send, cancel, grid click send, installed pack, search, idle, isolated data)')
    finally:
        terminal.cleanup()
