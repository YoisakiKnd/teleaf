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
