#!/usr/bin/env python3
"""Exercise legacy TDLib encryption recovery through a real terminal.
Only creates temporary databases; never supplies a Telegram phone or login code.
Run cargo build first. Requires the installed TDLib library.
"""
import base64
import ctypes
import fcntl
import json
import os
import pty
import re
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HASH = '0' * 32
KEY = ' original local key '


def create_fixture(folder):
    library_path = os.environ.get('TDLIB_PATH', str(ROOT / 'target/tdlib/libtdjson.dylib'))
    library = ctypes.CDLL(library_path)
    library.td_execute.argtypes = [ctypes.c_char_p]
    library.td_execute.restype = ctypes.c_char_p
    library.td_create_client_id.restype = ctypes.c_int
    library.td_send.argtypes = [ctypes.c_int, ctypes.c_char_p]
    library.td_receive.argtypes = [ctypes.c_double]
    library.td_receive.restype = ctypes.c_char_p
    library.td_execute(b'{"@type":"setLogVerbosityLevel","new_verbosity_level":0}')
    client = library.td_create_client_id()
    parameters = {
        '@type': 'setTdlibParameters', 'api_id': 1, 'api_hash': HASH,
        'database_directory': str(folder / 'tdlib'), 'files_directory': str(folder / 'files'),
        'database_encryption_key': base64.b64encode(KEY.encode()).decode(),
        'use_file_database': True, 'use_chat_info_database': True, 'use_message_database': True,
        'use_secret_chats': True, 'system_language_code': 'zh-CN', 'device_model': 'recovery-test',
        'system_version': sys.platform, 'application_version': '0.1.0',
    }
    library.td_send(client, json.dumps(parameters).encode())
    deadline = time.monotonic() + 10
    closing = False
    while time.monotonic() < deadline:
        response = library.td_receive(.1)
        if not response:
            continue
        value = json.loads(response)
        assert value.get('@type') != 'error', 'TDLib fixture initialization failed'
        state = value.get('authorization_state', {}).get('@type')
        if state == 'authorizationStateWaitPhoneNumber' and not closing:
            library.td_send(client, b'{"@type":"close"}')
            closing = True
        if state == 'authorizationStateClosed':
            assert closing and (folder / 'tdlib/td.binlog').exists()
            return
    raise AssertionError('TDLib fixture initialization timed out')


class Terminal:
    def __init__(self, folder, args=(), width=80, height=24, executable=None, image_protocol='halfblocks', environment=None):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack('HHHH', height, width, 0, 0))
            env = dict(os.environ, TG_DATA_DIR=str(folder), TG_IMAGE_PROTOCOL=image_protocol, TERM='xterm-256color')
            env.update(environment or {})
            for key in ('TG_API_ID', 'TG_API_HASH', 'TG_DB_KEY'):
                env.pop(key, None)
            os.execve(str(executable or ROOT / 'target/debug/teleaf'), ['teleaf', *args], env)
        self.output = bytearray()
        self.done = False

    def read(self, seconds=.2):
        end = time.monotonic() + seconds
        data = bytearray()
        while time.monotonic() < end:
            if select.select([self.fd], [], [], min(.05, max(0, end - time.monotonic())))[0]:
                try:
                    chunk = os.read(self.fd, 65536)
                except OSError:
                    break
                if not chunk:
                    break
                data.extend(chunk)
        self.output.extend(data)
        return re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]', b'', data)

    def wait(self, needle):
        data = bytearray()
        for _ in range(50):
            data.extend(self.read(.2))
            if needle.encode() in data:
                return
        raise AssertionError(f'Terminal did not display: {needle}')

    def send(self, data):
        os.write(self.fd, data)

    def paste(self, value):
        self.send(b'\x1b[200~' + value.encode() + b'\x1b[201~')
        self.read()

    def click(self, x, y):
        self.send(f'\x1b[<0;{x+1};{y+1}M\x1b[<0;{x+1};{y+1}m'.encode())

    def close(self):
        if self.done:
            return
        self.send(b'\x03')
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            self.read(.1)
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                assert os.waitstatus_to_exitcode(status) == 0
                assert KEY.encode() not in self.output
                assert b'wrong-local-key' not in self.output
                assert b'Client.cpp' not in self.output
                self.done = True
                os.close(self.fd)
                return
        raise AssertionError('Terminal did not exit cleanly')

    def cleanup(self):
        if not self.done:
            try:
                os.kill(self.pid, signal.SIGTERM)
                os.waitpid(self.pid, 0)
            except ProcessLookupError:
                pass
            os.close(self.fd)


def run():
    with tempfile.TemporaryDirectory(prefix='tg-recovery-smoke-') as temporary:
        folder = Path(temporary)
        template = folder / 'template'
        template.mkdir()
        subprocess.run([sys.executable, __file__, '--fixture', str(template)], check=True, timeout=15)
        saved = {'api_id': 1, 'api_hash': HASH}
        (template / 'config.json').write_text(json.dumps(saved))
        (template / 'files/local-only').write_bytes(b'preserved file')
        restore = folder / 'restore'
        shutil.copytree(template, restore)
        terminal = Terminal(restore)
        try:
            terminal.wait('旧本地密钥')
            terminal.paste(KEY)
            terminal.send(b'\r')
            terminal.wait('手机号')
            config = json.loads((restore / 'config.json').read_text())
            assert config['database_key'] == KEY and config['api_id'] == 1
            assert config.get('session') is None
            terminal.close()
        finally:
            terminal.cleanup()

        fresh = folder / 'fresh'
        shutil.copytree(template, fresh)
        (fresh / 'config.json').unlink()
        terminal = Terminal(fresh)
        try:
            terminal.wait('API ID')
            terminal.paste('1')
            terminal.send(b'\t')
            terminal.read()
            terminal.paste(HASH)
            terminal.send(b'\r')
            terminal.wait('旧本地密钥')
            terminal.paste('wrong-local-key')
            terminal.send(b'\r')
            terminal.wait('本地密钥不匹配')
            before = {path.relative_to(fresh): path.read_bytes() for path in fresh.rglob('*') if path.is_file()}
            # 80 x 24: recovery card buttons are in its final two rows.
            terminal.click(20, 18)
            terminal.wait('确认重新登录')
            assert not (fresh / 'sessions').exists(), 'Choosing the option must not change data yet'
            terminal.click(57, 18)
            terminal.wait('手机号')
            config = json.loads((fresh / 'config.json').read_text())
            session = fresh / 'sessions' / config['session']
            assert (session / 'tdlib/td.binlog').exists()
            assert config['database_key'] != 'wrong-local-key'
            assert config['api_id'] == 1 and config['api_hash'] == HASH
            assert (session / 'previous-config.json').read_bytes() == before[Path('config.json')]
            for path, content in before.items():
                if path != Path('config.json'):
                    assert (fresh / path).read_bytes() == content, f'Old data changed: {path}'
            terminal.close()
        finally:
            terminal.cleanup()
        terminal = Terminal(fresh)
        try:
            terminal.wait('手机号')
            assert '旧本地密钥'.encode() not in terminal.output
            terminal.close()
        finally:
            terminal.cleanup()
    print('PTY login recovery: PASS (original key, masked input, missing config, wrong key retry, mouse confirmation, old data/config preserved, new session survives restart)')


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--fixture':
        create_fixture(Path(sys.argv[2]))
    else:
        run()
