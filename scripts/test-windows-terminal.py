#!/usr/bin/env python3
"""Windows ConPTY input regression; no account data or phone/code submission.
Requires: cargo build; python -m pip install pywinpty==3.0.5 pyte==0.8.2
ConPTY exercises native console input, not Windows Terminal's GPU renderer.
"""
import os
import re
import select
import tempfile
import time
from pathlib import Path

from winpty import Backend, PtyProcess
import pyte

ROOT = Path(__file__).resolve().parents[1]
ANSI = re.compile(r'\x1b\[[0-?]*[ -/]*[@-~]')


class Terminal:
    def __init__(self, directory, demo, protocol):
        env = dict(os.environ, TG_DATA_DIR=str(directory), TG_IMAGE_PROTOCOL=protocol,
                   TERM='xterm-256color', TG_SYNC_OUTPUT='0')
        for name in ('TG_API_ID', 'TG_API_HASH', 'TG_DB_KEY'):
            env.pop(name, None)
        args = [str(ROOT / 'target/debug/teleaf.exe')]
        if demo:
            args.append('--demo')
        self.proc = PtyProcess.spawn(args, env=env, dimensions=(40, 110), backend=Backend.ConPTY)
        self.output = ''
        self.screen = pyte.Screen(110, 40)
        self.stream = pyte.Stream(self.screen)

    def read(self, seconds=.3):
        data = ''
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            remaining = deadline - time.monotonic()
            if select.select([self.proc.fileobj], [], [], min(.05, max(0, remaining)))[0]:
                try:
                    chunk = self.proc.read(65536)
                except EOFError:
                    break
                data += chunk
                self.stream.feed(chunk)
        self.output += data
        return ANSI.sub('', data)

    def send(self, keys):
        self.proc.write(keys)

    def wait(self, needle):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            self.read(.1)
            # ConPTY sends cursor-addressed diffs, often only the changed 开/关
            # glyph. Read the reconstructed screen rather than raw log text.
            # pyte.display can raise on an orphan wide-character continuation
            # after ConPTY overwrites half of an emoji. Read cell data directly;
            # empty continuation cells contribute no visible text.
            text = '\n'.join(
                ''.join(self.screen.buffer[y][x].data for x in range(self.screen.columns))
                for y in range(self.screen.lines)
            )
            if needle.replace(' ', '') in text.replace(' ', ''):
                return
        diagnostic = ANSI.sub('', self.output[-2000:])
        diagnostic = diagnostic.replace('private-test-hash', '<masked test value>')
        raise AssertionError(f'ConPTY did not display: {needle}; alive={self.proc.isalive()}; tail={diagnostic!r}')

    def close(self):
        self.send('\x11')  # Ctrl+Q: must remain available with mouse disabled.
        deadline = time.monotonic() + 5
        while self.proc.isalive() and time.monotonic() < deadline:
            self.read(.1)
        assert not self.proc.isalive(), 'ConPTY process did not exit after Ctrl+Q'
        assert self.proc.exitstatus == 0


def toggle_mouse_off(terminal):
    terminal.send('\x1bOS')  # F4's SS3 encoding, accepted by the ConPTY input parser.
    terminal.wait('设置与连接')
    terminal.send('\t\t\t\r')
    terminal.wait('鼠标：关')
    terminal.send('\x1b')
    terminal.read(.3)


def clipboard_regression():
    # Only write generated fixtures on disposable GitHub-hosted Windows runners.
    # Running the usual ConPTY suite locally never changes the user's clipboard.
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        print('Windows clipboard fixtures: SKIP (requires disposable CI runner)')
        return
    import ctypes
    import struct
    from ctypes import wintypes
    user = ctypes.WinDLL('user32', use_last_error=True)
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    user.CreateWindowExW.argtypes = [wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR,
                                   wintypes.DWORD, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                   ctypes.c_int, wintypes.HWND, wintypes.HANDLE,
                                   wintypes.HINSTANCE, ctypes.c_void_p]
    user.CreateWindowExW.restype = wintypes.HWND
    user.OpenClipboard.argtypes = [wintypes.HWND]
    user.SetClipboardData.argtypes = [wintypes.UINT, wintypes.HANDLE]
    user.SetClipboardData.restype = wintypes.HANDLE
    user.RegisterClipboardFormatW.argtypes = [wintypes.LPCWSTR]
    user.DestroyWindow.argtypes = [wintypes.HWND]
    kernel.GlobalAlloc.argtypes = [wintypes.UINT, ctypes.c_size_t]
    kernel.GlobalAlloc.restype = wintypes.HANDLE
    kernel.GlobalLock.argtypes = [wintypes.HANDLE]
    kernel.GlobalLock.restype = ctypes.c_void_p
    kernel.GlobalUnlock.argtypes = [wintypes.HANDLE]
    kernel.GlobalFree.argtypes = [wintypes.HANDLE]
    window = user.CreateWindowExW(0, 'STATIC', 'Teleaf CI clipboard', 0,
                                 0, 0, 0, 0, None, None, None, None)
    assert window, ctypes.WinError(ctypes.get_last_error())

    def put(formats):
        deadline = time.monotonic() + 2
        while not user.OpenClipboard(window):
            if time.monotonic() > deadline:
                raise ctypes.WinError(ctypes.get_last_error())
            time.sleep(.02)
        try:
            assert user.EmptyClipboard(), ctypes.WinError(ctypes.get_last_error())
            for format_id, data in formats:
                handle = kernel.GlobalAlloc(2, len(data))  # GMEM_MOVEABLE
                assert handle, ctypes.WinError(ctypes.get_last_error())
                pointer = kernel.GlobalLock(handle)
                assert pointer, ctypes.WinError(ctypes.get_last_error())
                ctypes.memmove(pointer, data, len(data))
                kernel.GlobalUnlock(handle)
                if not user.SetClipboardData(format_id, handle):
                    kernel.GlobalFree(handle)
                    raise ctypes.WinError(ctypes.get_last_error())
                # Windows owns the allocation after SetClipboardData succeeds.
        finally:
            user.CloseClipboard()

    def dib(width, height, bits, pixels):
        return struct.pack('<IiiHHIIiiII', 40, width, height, 1, bits, 0,
                           len(pixels), 0, 0, 0, 0) + pixels

    def staged():
        return set(Path(tempfile.gettempdir()).glob('teleaf-clipboard-*.png'))

    try:
        with tempfile.TemporaryDirectory(prefix='teleaf-clipboard-conpty-') as directory:
            terminal = Terminal(directory, True, 'halfblocks')
            try:
                terminal.wait('离线演示')
                terminal.send('\r')
                terminal.read(.5)
                png_format = user.RegisterClipboardFormatW('PNG')
                assert png_format
                fixtures = [
                    # Plain screenshot CF_DIB, no application-generated PNG/V5.
                    [(8, dib(1, 2, 24, bytes([255, 0, 0, 0, 0, 0, 255, 0])))],
                    # arboard prefers PNG and fails on it; the valid CF_DIB must
                    # still work through Teleaf's native fallback with mouse off.
                    [(png_format, b'not a PNG'),
                     (8, dib(1, -2, 32, bytes([0, 0, 255, 0, 255, 0, 0, 0])))],
                ]
                for index, formats in enumerate(fixtures):
                    if index:
                        terminal.send('\x1b[17~')  # F6: mouse off.
                        terminal.read(.2)
                    put(formats)
                    before = staged()
                    # F7 and a directly forwarded Ctrl+V both use the worker.
                    terminal.send('\x1b[18~' if index == 0 else '\x16')
                    terminal.wait('已加入附件')
                    terminal.wait('发送附件')
                    images = staged() - before
                    assert len(images) == 1, f'expected one staged image, got {len(images)}'
                    image = images.pop()
                    png = image.read_bytes()
                    assert png[:8] == b'\x89PNG\r\n\x1a\n'
                    assert struct.unpack('>II', png[16:24]) == (1, 2)
                    terminal.send('\x1b')  # Cancel without sending.
                    terminal.read(.3)
                    assert not image.exists(), 'cancelled screenshot was not removed'
                put([(png_format, b'invalid'), (8, b'invalid bitmap')])
                terminal.send('\x1b[18~')
                terminal.wait('CF_DIB 位图无法读取')
                put([(png_format, b'invalid'), (8, dib(8000, 8000, 32, b''))])
                terminal.send('\x1b[18~')
                terminal.wait('超过 1600 万像素')
                put([(13, 'q-clipboard-text\r\n第二行'.encode('utf-16-le') + b'\0\0')])
                terminal.send('\x1b[18~')
                terminal.wait('已粘贴剪贴板文字')
                terminal.wait('q-clipboard-text')
                terminal.wait('第二行')
                terminal.close()
                print('Windows clipboard: PASS (CF_DIB, bad PNG fallback, F7/Ctrl+V, '
                      'mouse off, cancel cleanup, malformed/large bitmap, multiline text)')
            finally:
                terminal.proc.close(force=True)
    finally:
        put([])
        user.DestroyWindow(window)


for demo, protocol in [(False, 'halfblocks'), (True, 'auto'), (True, 'sixel')]:
    with tempfile.TemporaryDirectory(prefix='teleaf-conpty-') as directory:
        terminal = Terminal(directory, demo, protocol)
        try:
            terminal.wait('离线演示' if demo else 'API ID')
            terminal.read(.5)
            toggle_mouse_off(terminal)
            if demo:
                terminal.send('\r')
                terminal.read(.5)
                terminal.send('i')
                terminal.read(.2)
                terminal.send('windows-keyboard-input')
                terminal.wait('windows-keyboard-input')
                terminal.send('\r')
                terminal.read(.3)
                terminal.send('q-followup-without-refocusing')
                terminal.wait('q-followup-without-refocusing')
                terminal.send('\r')
                terminal.read(.3)
                terminal.proc.pty.set_size(90, 30)
                terminal.screen.resize(lines=30, columns=90)
                terminal.read(.3)
            else:
                terminal.send('123456\t')
                terminal.read(.2)
                terminal.send('private-test-hash')
                terminal.wait('•')
                assert 'private-test-hash' not in terminal.output
                assert not (Path(directory) / 'config.json').exists()
            terminal.send('\x1bOS')
            terminal.wait('设置与连接')
            if protocol == 'sixel':
                # The headless ConPTY host may consume graphics; verify the
                # configured encoder and native keyboard path, not GPU output.
                assert 'Sixel' in terminal.read(.2) + ANSI.sub('', terminal.output)
            terminal.send('\x1b[Z\x1b[Z\r')  # Skip Notifications, then enable Mouse.
            terminal.wait('鼠标：开')
            terminal.send('\x1b[17~')  # F6 disables it again.
            terminal.wait('鼠标：关')
            terminal.send('\x1b')
            terminal.read(.3)
            terminal.close()
            print(f'Windows ConPTY: PASS (demo={demo}, protocol={protocol}, keyboard/mouse, resize, exit)')
        finally:
            terminal.proc.close(force=True)

clipboard_regression()
