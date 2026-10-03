#!/usr/bin/env python3
"""Install a verified TDLib runtime for source builds on macOS/Linux/Windows."""
import argparse
import sys
from pathlib import Path
from tdlib_runtime import ROOT, METADATA, install

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--dest', type=Path, default=ROOT / 'target/tdlib')
parser.add_argument('--platform', choices=METADATA['sha256'])
parser.add_argument('--archive', type=Path, help='Reuse a local archive; SHA-256 is still checked')
args = parser.parse_args()
try:
    print(f'已安装 {install(args.dest, args.platform, archive=args.archive)}')
except (OSError, ValueError) as error:
    print(f'TDLib 安装失败：{error}', file=sys.stderr)
    sys.exit(1)
