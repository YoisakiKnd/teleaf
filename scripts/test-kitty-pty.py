#!/usr/bin/env python3
"""Exercise Ghostty's Kitty protocol path using a simulated terminal capability reply.
Checks the native byte stream; visual GPU rendering still requires a real terminal.
Uses --demo with isolated data; never reads account configuration.
"""
import re
import runpy
import tempfile
import time
from pathlib import Path

root = Path(__file__).resolve().parents[1]
Terminal = runpy.run_path(str(root/'scripts/test-login-recovery-pty.py'))['Terminal']


def settle(terminal):
    # Full RGBA uploads can exceed 6 MB in a debug build; wait for queued encodes
    # and PTY backpressure to drain before asserting steady-state silence.
    deadline = time.monotonic() + 20
    quiet = 0
    # Triangle resampling of a detailed image can take more than 600 ms in
    # debug builds. A short gap between jobs is not steady state.
    while quiet < 7:
        before = len(terminal.output)
        terminal.read(.3)
        quiet = 0 if len(terminal.output) != before else quiet + 1
        assert time.monotonic() < deadline, 'Native frames kept reloading: ' + str(
            re.findall(rb'i=(\d+),a=T,U=1,f=32,t=d,s=(\d+),v=(\d+)', terminal.output)[-15:])


def assert_idle(terminal, message):
    before = len(terminal.output)
    terminal.read(1)
    assert len(terminal.output) == before, message


with tempfile.TemporaryDirectory(prefix='tg-kitty-smoke-') as folder:
    terminal = Terminal(folder, args=('--demo',), width=110, height=40, image_protocol='auto')
    try:
        deadline = time.monotonic() + 2
        while b'a=q' not in terminal.output:
            terminal.read(.01)
            assert time.monotonic() < deadline, 'Capability query was not emitted'
        terminal.send(b'\x1b_Gi=31;OK\x1b\\\x1b[6;40;20t\x1b[0n')
        terminal.wait('离线演示')
        terminal.read(.4)
        terminal.click(10,3)
        terminal.wait('图片直接放在这条消息里')
        settle(terminal)
        assert b'a=T,U=1' in terminal.output, 'Native Kitty image upload missing'
        terminal.send(b'kkkv')
        terminal.wait('媒体预览')
        settle(terminal)
        assert_idle(terminal, 'Idle native preview kept writing frames')
        for key in [b'+', b'+', b'\x1b[C', b'\x1b[D', b'-', b'0'] * 2:
            terminal.send(key)
            terminal.read(.25)
        settle(terminal)
        assert_idle(terminal, 'Zoom did not settle')
        uploads = re.findall(rb'i=(\d+),a=T,U=1,f=32,t=d,s=(\d+),v=(\d+)', terminal.output)
        assert any(int(w) > 640 for _,w,_ in uploads), 'Detailed preview stayed thumbnail-sized'
        assert all(int(w)*int(h) <= 1500000 and int(w) <= 2048 and int(h) <= 2048
                   for _,w,h in uploads), 'Native canvas escaped the pixel budget'
        active = False
        frames = 0
        for match in re.finditer(rb'\x1b\[\?2026([hl])|a=T,U=1', terminal.output):
            if match[1] == b'h':
                assert not active, 'Nested synchronized frame'
                active = True
                frames += 1
            elif match[1] == b'l':
                active = False
            else:
                assert active, 'Graphics upload was outside synchronized output'
        assert frames and not active
        terminal.send(b'\x1b')
        settle(terminal)
        assert_idle(terminal, 'Returning to inline images kept reloading')
        terminal.close()
        print('PTY Kitty: PASS (capability reply, native inline/detail uploads, bounded pixels, synchronized frames, zoom/pan, settled idle, return to chat, isolated data)')
    finally:
        terminal.cleanup()
