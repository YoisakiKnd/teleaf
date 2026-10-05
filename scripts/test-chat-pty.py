#!/usr/bin/env python3
"""Test chat mouse controls through the native event parser, using --demo.
No account config or TDLib is needed. Run cargo build first.
"""
import re
import runpy
import tempfile
from pathlib import Path

root = Path(__file__).resolve().parents[1]
Terminal = runpy.run_path(str(root/'scripts/test-login-recovery-pty.py'))['Terminal']
with tempfile.TemporaryDirectory(prefix='tg-chat-smoke-') as folder:
    terminal = Terminal(folder, args=('--demo',), width=110, height=40)
    try:
        terminal.wait('离线演示')
        terminal.read(.4)
        modes = re.findall(rb'\x1b\[\?(1000|1002|1003)([hl])', terminal.output)
        mode = None
        # Simulate terminals that retain one active mouse tracking mode.
        for number, enabled in modes:
            mode = number if enabled == b'h' else None
        assert mode == b'1002', f'Button tracking was disabled: {modes}'
        terminal.click(15,2)  # Work folder tab
        terminal.wait('工作')
        terminal.click(10,3)  # First chat
        terminal.wait('图片直接放在这条消息里')
        terminal.read(.7)
        plain = re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]', b'', terminal.output)
        assert '陈同学'.encode() in plain, 'Group sender name did not render'
        assert '产品讨论'.encode() in plain, 'Group title did not render'
        assert '▄'.encode() in terminal.output or '▀'.encode() in terminal.output, 'Inline fallback image did not render'
        assert '图片 / 贴纸预览'.encode() not in terminal.output, 'Old separate preview strip returned'
        terminal.click(48,25)  # Sticker inside the timeline
        terminal.wait('媒体预览')
        terminal.send(b'\x1b')
        terminal.read(.3)
        terminal.click(50,31)  # Select latest message before its contextual buttons appear
        terminal.read(.2)
        terminal.click(99,29)  # Reply button on the latest message
        terminal.wait('回复消息')
        terminal.send(b'\x1b')
        terminal.read(.3)
        terminal.click(105,29)  # Message menu button
        terminal.wait('消息操作')
        terminal.send(b'\x1b')
        terminal.read(.3)
        terminal.click(50,36)  # Composer
        terminal.paste('mouse-send-demo')
        terminal.click(98,38)  # Send button
        sent = terminal.read(.6)
        assert b'mouse-send-demo' in sent, 'Mouse send did not add a chat message'
        terminal.click(34,2)  # Folder selector
        terminal.wait('Telegram 分组')
        terminal.send(b'\x1b')
        terminal.read(.3)
        terminal.send(b'\x1b[<64;50;10M' * 4)  # Wheel up inside the timeline
        terminal.read(.5)
        terminal.read(.3)
        idle = terminal.read(.5)
        assert not idle, f'Idle caused {len(idle)} output bytes'
        terminal.send(b'\x1b[<35;50;10M' * 100)
        assert not terminal.read(.3), 'Idle pointer motion caused redraws'
        assert not (Path(folder)/'config.json').exists(), 'Demo touched account configuration'
        terminal.close()
        print('PTY chat mouse: PASS (tracking mode, folder/chat clicks, inline image fallback, image modal, message menu, composer/send, folder picker, wheel, idle/motion without redraw, no account config)')
    finally:
        terminal.cleanup()
