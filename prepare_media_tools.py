"""Developer packaging step. End users use the native Rust first-use installer.

Builds the pinned official whisper.cpp CLI (small, static arm64 + Apple frameworks),
bundles only that executable and licenses, and verifies the external model/runtime.
Never accesses browser profiles or downloads private media.
"""
from pathlib import Path
from urllib.request import Request, urlopen
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
BUILD = ROOT / 'qa/media-tools-build'
COMMIT = 'd1be6fde11ac6e0407606b4e42fe72d34add8037'
ASSETS = [
    ('yt-dlp_macos', 'https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp_macos', '0f192b7ec147ab6288885d6351d9ab67367640029b4377576ef46dd79cf7b202', 37146048),
    ('ggml-small-q5_1.bin', 'https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin', 'ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb', 190085487),
]

def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as file:
        while True:
            data = file.read(1024 * 1024)
            if not data:
                break
            value.update(data)
    return value.hexdigest()

def asset(folder, name, url, expected, size):
    path = folder / name
    if path.is_file() and path.stat().st_size == size and digest(path) == expected:
        return path
    temporary = folder / (name + '.partial')
    with urlopen(Request(url, headers={'User-Agent': 'Graffin-Studio/2 asset preparation'}), timeout=90) as response, temporary.open('wb') as output:
        shutil.copyfileobj(response, output, 1024 * 1024)
    if temporary.stat().st_size != size or digest(temporary) != expected:
        temporary.unlink(missing_ok=True)
        raise RuntimeError('Official asset checksum mismatch: ' + name)
    os.replace(temporary, path)
    return path

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--no-build', action='store_true')
    args = parser.parse_args()
    BUILD.mkdir(parents=True, exist_ok=True)
    source = BUILD / 'whisper.cpp'
    if not source.is_dir():
        subprocess.run(['git', 'clone', '--depth', '1', '--branch', 'v1.9.5', 'https://github.com/ggml-org/whisper.cpp.git', str(source)], check=True)
    actual = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    if actual != COMMIT:
        raise RuntimeError('Unexpected whisper.cpp commit')
    cmake = shutil.which('cmake')
    if not cmake:
        cmake = str(BUILD / 'cmake/cmake/data/bin/cmake')
        if not Path(cmake).is_file():
            subprocess.run([sys.executable, '-m', 'pip', 'install', '--target', str(BUILD / 'cmake'), 'cmake==4.4.4'], check=True)
    if not args.no_build:
        subprocess.run([cmake, '-S', str(source), '-B', str(source / 'build'), '-DCMAKE_BUILD_TYPE=Release', '-DBUILD_SHARED_LIBS=OFF', '-DGGML_METAL=OFF', '-DGGML_NATIVE=OFF', '-DWHISPER_BUILD_TESTS=OFF', '-DWHISPER_BUILD_EXAMPLES=ON', '-DCMAKE_OSX_DEPLOYMENT_TARGET=13.5'], check=True)
        subprocess.run([cmake, '--build', str(source / 'build'), '--target', 'whisper-cli', '--config', 'Release', '-j', '6'], check=True)
    binary = source / 'build/bin/whisper-cli'
    resources = ROOT / 'src-tauri/resources/media-tools'
    resources.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(binary, resources / 'whisper-cli')
    (resources / 'whisper-cli').chmod(0o755)
    shutil.copyfile(source / 'LICENSE', resources / 'Whisper-LICENSE.txt')
    cache = Path.home() / 'Library/Application Support/com.barsik.studio/media-tools'
    cache.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(binary, cache / 'whisper-cli')
    (cache / 'whisper-cli').chmod(0o755)
    for name, url, expected, size in ASSETS:
        path = asset(cache, name, url, expected, size)
        if name == 'yt-dlp_macos':
            path.chmod(0o755)
    report = {
        'whisperVersion': 'v1.9.5', 'whisperCommit': COMMIT,
        'whisperBytes': binary.stat().st_size, 'whisperSha256': digest(binary),
        'architecture': 'arm64', 'backends': ['CPU', 'Accelerate'],
        'cache': str(cache),
        'externalAssets': [{'name': name, 'url': url, 'sha256': expected, 'bytes': size} for name, url, expected, size in ASSETS],
        'sources': ['https://github.com/ggml-org/whisper.cpp', 'https://github.com/yt-dlp/yt-dlp', 'https://huggingface.co/ggerganov/whisper.cpp'],
    }
    (resources / 'runtime-manifest.json').write_text(json.dumps({key: value for key, value in report.items() if key != 'cache'}, indent=2) + '\n')
    (ROOT / 'qa/media-tools-runtime.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))

if __name__ == '__main__':
    main()
