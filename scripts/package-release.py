#!/usr/bin/env python3
"""Build a relocatable release archive including TDLib and non-system runtimes."""
import argparse
import gzip
import os
import re
import shutil
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path
from tdlib_runtime import ROOT, METADATA, checksum, configure_console, current_platform, install


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def macos_dependencies(folder):
    pending = list(folder.glob('*.dylib'))
    visited = set()
    while pending:
        library = pending.pop()
        if library.name in visited:
            continue
        visited.add(library.name)
        library.chmod(0o755)
        # The first library entry is its install name, not an external dependency.
        dependencies = command('otool', '-L', str(library)).splitlines()[2:]
        for line in dependencies:
            original = line.strip().split(' (')[0]
            if original.startswith(('/usr/lib/', '/System/')):
                continue
            source = Path(original)
            target = folder / source.name
            if not target.exists():
                if not source.is_absolute() or not source.is_file():
                    raise RuntimeError(f'无法解析 macOS 运行库依赖：{original}')
                shutil.copyfile(source, target)
                pending.append(target)
            subprocess.run(['install_name_tool', '-change', original, '@loader_path/' + target.name, str(library)], check=True)
        subprocess.run(['install_name_tool', '-id', '@loader_path/' + library.name, str(library)], check=True)
    for library in folder.glob('*.dylib'):
        subprocess.run(['codesign', '--force', '--sign', '-', str(library)], check=True)


def linux_dependencies(folder):
    primary = folder / 'libtdjson.so'
    output = command('ldd', str(primary))
    if 'not found' in output:
        raise RuntimeError('TDLib 缺少构建机运行库，请安装 libc++1-18/libc++abi1-18 等：\n' + output)
    # Keep glibc and loader supplied by the OS; copy the remainder by SONAME.
    system = re.compile(r'^(libc|libm|libpthread|libdl|librt|libresolv)\.so')
    for line in output.splitlines():
        match = re.match(r'\s*(\S+) => (/\S+) \(', line)
        if match and not system.match(match[1]):
            shutil.copyfile(match[2], folder / match[1])
    for library in folder.iterdir():
        subprocess.run(['patchelf', '--set-rpath', '$ORIGIN', str(library)], check=True)


def windows_crt(staging):
    base = Path(os.environ.get('ProgramFiles', 'C:/Program Files'))
    directories = sorted(base.glob('Microsoft Visual Studio/*/*/VC/Redist/MSVC/*/x64/Microsoft.VC*.CRT'))
    if not directories:
        raise RuntimeError('未找到 MSVC x64 可再分发运行库；请在安装了 Visual Studio C++ 的构建机打包')
    for library in directories[-1].glob('*.dll'):
        shutil.copyfile(library, staging / library.name)


def write_archive(staging, archive):
    paths = sorted(p for p in staging.rglob('*') if p.is_file())
    if archive.name.endswith('.zip'):
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as output:
            for path in paths:
                info = zipfile.ZipInfo(path.relative_to(staging).as_posix())
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = (0o755 if path.name == 'teleaf.exe' else 0o644) << 16
                with path.open('rb') as source, output.open(info, 'w') as target:
                    shutil.copyfileobj(source, target, 1024 * 1024)
    else:
        with archive.open('wb') as stream, gzip.GzipFile(fileobj=stream, mode='wb', filename='', mtime=0) as zipped, tarfile.open(fileobj=zipped, mode='w') as output:
            for path in paths:
                info = output.gettarinfo(str(path), arcname=path.relative_to(staging).as_posix())
                info.uid = info.gid = info.mtime = 0
                info.uname = info.gname = ''
                with path.open('rb') as stream:
                    output.addfile(info, stream)


def package(binary, name, version, output, archive=None):
    if name != current_platform():
        raise ValueError('运行库打包和加载测试必须在同架构的本机构建机运行')
    if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?', version):
        raise ValueError('无效版本号')
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='teleaf-package-') as temporary:
        staging = Path(temporary)
        runtime = staging / 'tdlib'
        install(runtime, name, archive=archive)
        executable = staging / ('teleaf.exe' if name.startswith('windows-') else 'teleaf')
        shutil.copyfile(binary, executable)
        executable.chmod(0o755)
        if name.startswith('macos-'):
            macos_dependencies(runtime)
            subprocess.run(['codesign', '--force', '--sign', '-', str(executable)], check=True)
        elif name.startswith('linux-'):
            linux_dependencies(runtime)
        else:
            windows_crt(staging)
        shutil.copyfile(ROOT / 'README.md', staging / 'README.md')
        shutil.copytree(ROOT / 'packaging/licenses', staging / 'LICENSES')
        if (ROOT / 'LICENSE').exists():
            shutil.copyfile(ROOT / 'LICENSE', staging / 'LICENSE')
        # Verify outside the project cwd, with no override pointing at developer libraries.
        environment = dict(os.environ)
        for key in ('TDLIB_PATH', 'LD_LIBRARY_PATH', 'DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH'):
            environment.pop(key, None)
        subprocess.run([str(executable), '--check'], cwd=staging, env=environment, check=True)
        extension = 'zip' if name.startswith('windows-') else 'tar.gz'
        result = output / f'teleaf-{version}-{name}.{extension}'
        write_archive(staging, result)
    result.with_name(result.name + '.sha256').write_text(f'{checksum(result)}  {result.name}\n', encoding='utf-8')
    print(result)
    return result


if __name__ == '__main__':
    configure_console()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--platform', choices=METADATA['sha256'], default=current_platform())
    parser.add_argument('--version', required=True)
    parser.add_argument('--output', type=Path, default=ROOT / 'target/dist')
    parser.add_argument('--archive', type=Path)
    args = parser.parse_args()
    package(args.binary, args.platform, args.version, args.output, args.archive)
