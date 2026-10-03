#!/usr/bin/env python3
"""Repeatable offline memory scenario, or read-only sampling of an existing PID.
On macOS report physical footprint/peak; on Linux report RSS/peak RSS instead.
No account configuration is read by the offline scenario.
"""
import argparse
import json
import re
import runpy
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def large_fixture(path):
    """Write a 12 MP RGB gradient row by row; allocations belong to this script."""
    width, height = 4000, 3000
    compressor = zlib.compressobj()
    def chunk(stream, kind, data):
        stream.write(struct.pack('>I', len(data)) + kind + data
                     + struct.pack('>I', zlib.crc32(kind + data)))
    with path.open('wb') as stream:
        stream.write(b'\x89PNG\r\n\x1a\n')
        chunk(stream, b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
        for y in range(height):
            row = bytes(v for x in range(width) for v in
                        (x * 255 // (width-1), y * 255 // (height-1), (x+y) % 256))
            data = compressor.compress(b'\0' + row)
            if data:
                chunk(stream, b'IDAT', data)
        chunk(stream, b'IDAT', compressor.flush())
        chunk(stream, b'IEND', b'')


def settle(terminal):
    deadline = time.monotonic() + 30
    quiet = 0
    while quiet < 10:
        before = len(terminal.output)
        terminal.read(.2)
        quiet = quiet + 1 if len(terminal.output) == before else 0
        if time.monotonic() > deadline:
            raise RuntimeError('Image output did not settle')


def memory(pid):
    if sys.platform == 'darwin':
        result = subprocess.run(['vmmap', '-summary', str(pid)],
                                check=True, capture_output=True, text=True)
        values = re.findall(r'Physical footprint(?: \(peak\))?:\s+([\d.]+)([KMG])', result.stdout)
        if len(values) != 2:
            raise RuntimeError('vmmap did not return physical footprint')
        current, peak = [float(number) * {'K': 1/1024, 'M': 1, 'G': 1024}[unit]
                         for number, unit in values]
        return {'physical_MiB': round(current, 2), 'peak_MiB': round(peak, 2)}
    status = Path(f'/proc/{pid}/status').read_text()
    return {label: round(int(re.search(rf'{field}:\s+(\d+)', status)[1])/1024, 2)
            for field, label in [('VmRSS', 'rss_MiB'), ('VmHWM', 'peak_rss_MiB')]}


def sample_process(pid):
    elapsed = subprocess.check_output(['ps', '-p', str(pid), '-o', 'time='], text=True).strip()
    seconds = 0.0
    for part in elapsed.split(':'):
        seconds = seconds * 60 + float(part)
    return {**memory(pid), 'cpu_seconds': round(seconds, 2)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    target = parser.add_mutually_exclusive_group()
    target.add_argument('--pid', type=int, help='Measure an existing process without controlling it')
    target.add_argument('--binary', type=Path, default=ROOT/'target/debug/teleaf')
    parser.add_argument('--protocol', choices=['halfblocks', 'kitty', 'sixel', 'iterm2'], default='halfblocks')
    parser.add_argument('--cell-size', default='20x40')
    parser.add_argument('--large-image', action='store_true', help='Replace the isolated demo detail with a 4000x3000 PNG')
    args = parser.parse_args()
    if args.pid:
        print(json.dumps({'pid': args.pid, **memory(args.pid)}, ensure_ascii=False))
        return
    Terminal = runpy.run_path(str(ROOT/'scripts/test-login-recovery-pty.py'))['Terminal']
    results = []
    with tempfile.TemporaryDirectory(prefix='tg-memory-') as folder:
        fixture = Path(folder)/'large.png'
        if args.large_image:
            large_fixture(fixture)
        terminal = Terminal(folder, args=('--demo',), width=180, height=44,
                            executable=args.binary.resolve(), image_protocol=args.protocol,
                            environment={'TG_CELL_SIZE': args.cell_size})
        try:
            terminal.wait('离线演示')
            for _ in range(50):
                rendered = re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]', b'', terminal.output)
                if '点击打开离线演示会话'.encode() in rendered:
                    break
                terminal.read(.1)
            else:
                raise RuntimeError('Demo chat list was not ready')
            settle(terminal)
            if args.large_image:
                # Only this script's isolated --demo process owns this fixture.
                shutil.copyfile(fixture, Path(tempfile.gettempdir())/f'tg-tui-demo-{terminal.pid}'/'detail.png')
            results.append({'stage': 'chat list', **sample_process(terminal.pid)})
            terminal.click(10, 3)
            terminal.wait('图片直接放在这条消息里')
            settle(terminal)
            results.append({'stage': 'inline photo/sticker', **sample_process(terminal.pid)})
            terminal.send(b'kkkv')  # Select the photo, then open its modal.
            terminal.wait('媒体预览')
            settle(terminal)
            results.append({'stage': 'photo modal', **sample_process(terminal.pid)})
            for key in [b'+', b'+', b'\x1b[C', b'\x1b[D', b'-', b'0'] * 2:
                terminal.send(key)
                terminal.read(.2)
            settle(terminal)
            results.append({'stage': 'zoom/pan repeatedly', **sample_process(terminal.pid)})
            terminal.send(b'\x1b')
            settle(terminal)
            results.append({'stage': 'return to chat', **sample_process(terminal.pid)})
            terminal.close()
        finally:
            terminal.cleanup()
    print(json.dumps({'binary': str(args.binary), 'mode': f'offline, {args.protocol}, 180x44, cell {args.cell_size}',
                      'detail_pixels': '4000x3000' if args.large_image else '1280x640',
                      'stages': results}, indent=2, ensure_ascii=False))


if __name__ == '__main__':
    main()
