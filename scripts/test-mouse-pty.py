#!/usr/bin/env python3
"""Native mouse smoke test (macOS/Linux). Run cargo build first.
Uses isolated temporary data, invalid credentials, and no Telegram login.
"""
import re, shutil, os, pty, fcntl, termios, struct, select, time, tempfile, signal
from pathlib import Path
root = Path(__file__).resolve().parents[1]
folder = tempfile.mkdtemp(prefix='tg-mouse-smoke-')
pid, fd = pty.fork()
if pid == 0:
    fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack('HHHH',24,80,0,0))
    env = dict(os.environ, TG_DATA_DIR=folder, TG_IMAGE_PROTOCOL='halfblocks', TERM='xterm-256color')
    for key in ('TG_API_ID','TG_API_HASH','TG_DB_KEY'): env.pop(key, None)
    os.execve(str(root/'target/debug/teleaf'), ['teleaf'], env)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH',24,80,0,0))
output = bytearray()
def read(seconds):
    data = bytearray(); end = time.monotonic() + seconds
    while time.monotonic() < end:
        if select.select([fd], [], [], min(.05, max(0,end-time.monotonic())))[0]:
            try: chunk = os.read(fd, 65536)
            except OSError: break
            if not chunk: break
            data.extend(chunk)
    output.extend(data); return bytes(data)
def send(data): os.write(fd, data); return read(.15)
def click(x,y): return send(f'\x1b[<0;{x+1};{y+1}M\x1b[<0;{x+1};{y+1}m'.encode())
try:
    first = read(1.5)
    assert 'API ID'.encode() in first, repr(first[:1500])
    click(10,11)
    typed = send(b'\x1b[200~mouse-secret\x1b[201~')
    assert '•'.encode() in typed and b'mouse-secret' not in output
    rejected = click(61,21)
    assert 'API ID'.encode() in rejected
    assert not (Path(folder)/'config.json').exists()
    settings = click(65,0)
    assert '设置与连接'.encode() in re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]', b'', settings)
    # Disable capture using the clickable preference; restore through F6.
    off = click(30,20)
    assert b'\x1b[?1000l' in off
    on = send(b'\x1b[17~')
    assert b'\x1b[?1000h' in on
    click(72,1)
    read(.3)
    # Movement reports produce no writes/redraws.
    movement = send(b'\x1b[<35;10;15M' * 100)
    assert not movement, f'movement caused output: {len(movement)} bytes'
    idle = read(.5)
    assert not idle, f'idle caused output: {len(idle)} bytes'
    send(b'\x03')
    _, status = os.waitpid(pid, 0)
    assert os.waitstatus_to_exitcode(status) == 0
    assert b'Client.cpp' not in output
    print('PTY mouse smoke: PASS (API field focus, masked paste, validation button, settings, capture toggle/F6, close, motion and idle with zero output, clean exit)')
finally:
    try: os.kill(pid, signal.SIGTERM)
    except ProcessLookupError: pass
    os.close(fd)
    try: os.waitpid(pid, 0)
    except ChildProcessError: pass
    shutil.rmtree(folder)
