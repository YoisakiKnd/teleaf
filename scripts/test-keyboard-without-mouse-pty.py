#!/usr/bin/env python3
"""Regression: settings, login input and chat input without mouse capture.
Uses an isolated data directory and an offline demo; never submits credentials.
Run cargo build first. Requires macOS/Linux and a build without bundled API keys.
"""
import tempfile
from pathlib import Path
from importlib.machinery import SourceFileLoader

Terminal = SourceFileLoader(
    'recovery', str(Path(__file__).with_name('test-login-recovery-pty.py'))
).load_module().Terminal


def settings_mouse_off(terminal):
    terminal.send(b'\x1b[14~')  # F4
    terminal.wait('设置与连接')
    terminal.send(b'\t\t\t\r')  # Tab to Mouse, Enter
    terminal.wait('鼠标：关')
    assert b'\x1b[?1000l' in terminal.output


def restore_from_settings(terminal):
    terminal.send(b'\x1b[14~')
    terminal.wait('设置与连接')
    terminal.send(b'\x1b[Z\r')  # Shift+Tab selects the last button
    terminal.wait('鼠标：开')
    assert b'\x1b[?1000h' in terminal.output
    terminal.send(b'\x1b')
    terminal.read(.3)


with tempfile.TemporaryDirectory(prefix='teleaf-keyboard-') as folder:
    terminal = Terminal(Path(folder))
    try:
        terminal.wait('API ID')
        settings_mouse_off(terminal)
        terminal.send(b'\x1b')
        terminal.read(.3)
        terminal.send(b'123456\t')
        terminal.read(.3)
        terminal.send(b'keyboard-private-hash')
        masked = terminal.read(.3)
        assert '•'.encode() in masked
        assert b'keyboard-private-hash' not in terminal.output
        assert not (Path(folder) / 'config.json').exists()
        restore_from_settings(terminal)
        terminal.close()
    finally:
        terminal.cleanup()

with tempfile.TemporaryDirectory(prefix='teleaf-keyboard-demo-') as folder:
    terminal = Terminal(Path(folder), args=('--demo',), width=110, height=40)
    try:
        terminal.wait('Teleaf')
        terminal.read(.5)
        settings_mouse_off(terminal)
        terminal.send(b'\x1b')
        terminal.read(.3)
        terminal.send(b'\r')  # Open the selected conversation.
        terminal.read(.3)
        terminal.send(b'i')
        terminal.read(.3)
        terminal.send(b'keyboard-without-mouse')
        terminal.wait('keyboard-without-mouse')
        terminal.send(b'\r')
        terminal.read(.3)
        terminal.send(b'q-followup-without-refocusing')
        terminal.wait('q-followup-without-refocusing')
        terminal.send(b'\r')
        terminal.read(.3)
        restore_from_settings(terminal)
        terminal.close()
    finally:
        terminal.cleanup()

print('PTY keyboard without mouse: PASS (settings Tab/Shift+Tab/Enter, login typing/masking, consecutive sends retain composer, Esc, clean exit)')
