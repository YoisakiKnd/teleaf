#!/usr/bin/env python3
"""Simulate Sixel and cell-size replies; check inline/preview/scroll byte streams.
Uses isolated offline demo data. Does not test an emulator's GPU renderer.
"""
import re
import runpy
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
Terminal = runpy.run_path(str(ROOT / 'scripts/test-login-recovery-pty.py'))['Terminal']


def settle(terminal):
    deadline = time.monotonic() + 20
    quiet = 0
    while quiet < 5:
        before = len(terminal.output)
        terminal.read(.3)
        quiet = quiet + 1 if len(terminal.output) == before else 0
        assert time.monotonic() < deadline, 'Sixel frames did not settle'


with tempfile.TemporaryDirectory(prefix='teleaf-sixel-') as directory:
    terminal = Terminal(directory, args=('--demo',), width=110, height=40,
                        image_protocol='auto', environment={'WT_SESSION': 'test'})
    try:
        deadline = time.monotonic() + 2
        while b'\x1b[16t' not in terminal.output:
            terminal.read(.01)
            assert time.monotonic() < deadline, 'Capability query missing'
        terminal.send(b'\x1b[?62;4;22c\x1b[6;24;12t\x1b[0n')
        terminal.wait('离线演示')
        terminal.read(.5)
        terminal.click(10, 3)
        terminal.wait('图片直接放在这条消息里')
        settle(terminal)
        sixels = re.findall(rb'\x1bP[^\x1b]*q[^\x1b]*\x1b\\\x1b\[\d+;\d+H', terminal.output)
        assert sixels, 'Sixel upload with restored cursor missing'
        terminal.send(b'kkkv')
        terminal.wait('媒体预览')
        settle(terminal)
        for keys in (b'+', b'\x1b[C', b'0', b'\x1b'):
            terminal.send(keys)
            terminal.read(.3)
        settle(terminal)
        before = len(terminal.output)
        terminal.read(1)
        assert len(terminal.output) == before, 'Idle Sixel view kept repainting'
        terminal.send(b'\x1b[<64;50;10M' * 3)
        settle(terminal)
        terminal.close()
        print('PTY Sixel: PASS (auto/cell-size replies, inline/detail, cursor restore, zoom/pan, scroll, idle, exit)')
    finally:
        terminal.cleanup()
