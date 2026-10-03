"""Pinned TDLib downloader shared by source installation and release packaging."""
import hashlib
import json
import os
import platform
import re
import tempfile
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
METADATA = json.loads((ROOT / 'packaging/tdlib.json').read_text())


def current_platform():
    system = {'Darwin': 'macos', 'Linux': 'linux', 'Windows': 'windows'}.get(platform.system())
    machine = platform.machine().lower()
    arch = {'arm64': 'aarch64', 'aarch64': 'aarch64', 'amd64': 'x86_64', 'x86_64': 'x86_64'}.get(machine)
    name = f'{system}-{arch}'
    if name not in METADATA['sha256']:
        raise ValueError(f'不支持的平台：{platform.system()} / {machine}；请设置 TDLIB_PATH')
    return name


def checksum(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def extract(archive, destination, name, expected):
    """Never extract arbitrary paths, symlinks, headers or static libraries."""
    if checksum(archive) != expected:
        raise ValueError('TDLib SHA-256 不匹配，已拒绝安装')
    destination = Path(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    primary = {'macos': 'libtdjson.dylib', 'linux': 'libtdjson.so', 'windows': 'tdjson.dll'}[name.split('-')[0]]
    with zipfile.ZipFile(archive) as source, tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
        selected = []
        for member in source.infolist():
            path = PurePosixPath(member.filename)
            allowed = (path.parent.as_posix() == 'tdlib/bin' and re.fullmatch(r'[A-Za-z0-9_.+-]+\.dll', path.name)) if name.startswith('windows-') else member.filename == f'tdlib/lib/{primary}'
            if allowed:
                if member.file_size > 256 * 1024 * 1024:
                    raise ValueError('TDLib 发布包成员超过大小限制')
                target = Path(temporary) / path.name
                with source.open(member) as stream, target.open('wb') as output:
                    while block := stream.read(1024 * 1024):
                        output.write(block)
                selected.append(target)
        if not any(path.name == primary and path.stat().st_size for path in selected):
            raise ValueError(f'TDLib 发布包缺少 {primary}')
        destination.mkdir(parents=True, exist_ok=True)
        for path in selected:
            os.replace(path, destination / path.name)
    return destination / primary


def install(destination, name=None, cache=None, archive=None):
    name = name or current_platform()
    expected = METADATA['sha256'][name]
    filename = f"tdlib-{METADATA['version']}-{name}.zip"
    cache = Path(cache or ROOT / 'target/tdlib')
    if archive is None:
        cache.mkdir(parents=True, exist_ok=True)
        archive = cache / filename
        if not archive.exists() or checksum(archive) != expected:
            partial = archive.with_suffix('.zip.part')
            try:
                print(f"下载 TDLib {METADATA['version']} ({name})…", flush=True)
                with urllib.request.urlopen(f"{METADATA['release']}/{filename}", timeout=60) as response, partial.open('wb') as output:
                    while block := response.read(1024 * 1024):
                        output.write(block)
                if checksum(partial) != expected:
                    raise ValueError('TDLib SHA-256 不匹配，已拒绝安装')
                os.replace(partial, archive)
            finally:
                partial.unlink(missing_ok=True)
    return extract(archive, destination, name, expected)
