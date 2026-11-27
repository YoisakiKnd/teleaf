#!/usr/bin/env python3
"""Offline PTY regression: attachments and Telegram-like sticker tray."""
import re
import runpy
import struct
import tempfile
import zlib
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
    photo = Path(folder) / 'photo 🖼️.png'
    def png_chunk(kind, data):
        return (struct.pack('>I', len(data)) + kind + data
                + struct.pack('>I', zlib.crc32(kind + data)))
    photo.write_bytes(
        b'\x89PNG\r\n\x1a\n'
        + png_chunk(b'IHDR', struct.pack('>IIBBBBB', 2, 1, 8, 6, 0, 0, 0))
        + png_chunk(b'IDAT', zlib.compress(b'\0' + b'\xff\0\0\xff' * 2))
        + png_chunk(b'IEND', b'')
    )
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
        terminal.wait('发送原文件')
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
        terminal.wait('发送原文件')
        terminal.send(b'\x1b')
        terminal.read(.3)
        terminal.send(b'\tp')  # Browse messages and open compressed photo mode.
        terminal.wait('发送图片')
        terminal.paste(f'"{photo}"')
        assert '2×1'.encode() in plain(terminal.output), 'Selected photo dimensions are missing'
        terminal.send(b'\x1b[15~')  # F5 selects original file.
        terminal.read(.3)
        terminal.send(b'\x1b[15~')  # F5 returns to compressed photo.
        terminal.read(.3)
        terminal.send(b'\x1b[19~')  # F8. PNG should be prepared as JPEG.
        terminal.wait('.jpg')
        terminal.send(b't')
        terminal.wait('贴纸 · 点击发送')
        terminal.read(.5)
        terminal.click(48,26) # First thumbnail above the composer
        terminal.wait('贴纸已提交')
        terminal.read(.3)
        terminal.click(68,21) # Installed pack tab above the status row
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
        print('PTY attachments/stickers: PASS (Ctrl+O, quoted multi-path paste, caption, F8 grouped send, photo compression and F5, cancel, grid click send, installed pack, search, idle, isolated data)')
    finally:
        terminal.cleanup()
