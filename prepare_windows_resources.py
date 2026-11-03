"""Stage pinned, redistributable Windows x64 media tools for Graffin Studio.

Downloads go to an external cache. Only explicitly selected archive members enter
the application resources. Every executable archive is checked before use.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import urllib.request
import zipfile
import xml.etree.ElementTree as ET


ASSETS = {
    "ffmpeg": {
        "version": "9.0.2",
        "filename": "ffmpeg-9.0.2-essentials_build.zip",
        "url": "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip",
        "sha256": "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba",
        "bytes": 114768076,
        "checksumSource": "https://api.github.com/repos/GyanD/codexffmpeg/releases/tags/9.0.2",
    },
    "whisper": {
        "version": "1.8.2",
        "filename": "whisper-bin-x64-v1.8.2.zip",
        "url": "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.2/whisper-bin-x64.zip",
        "sha256": "b1514ebc099765e39fa37eb780b92a140a94c86bb0b3b3d98226b38825979732",
        "bytes": 3832432,
        "checksumSource": "https://api.github.com/repos/ggml-org/whisper.cpp/releases/tags/v1.8.2",
    },
    "node": {
        "version": "24.21.0",
        "filename": "node-v24.21.0-win-x64.zip",
        "url": "https://nodejs.org/dist/v24.21.0/node-v24.21.0-win-x64.zip",
        "sha256": "158f7685b44de51f6c0df1d153526cbcd3e1bc739a8dfc607721cef75de9e541",
        "checksumSource": "https://nodejs.org/dist/v24.21.0/SHASUMS256.txt",
    },
    "ytDlp": {
        "version": "2026.08.19",
        "filename": "yt-dlp-2026.08.19.exe",
        "url": "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp.exe",
        "sha256": "66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a",
        "bytes": 17840399,
        "checksumSource": "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/SHA2-256SUMS",
    },
    "visualCRuntime": {
        "version": "14.51.36247.0",
        "filename": "VC_redist.x64.exe",
        "url": "https://download.visualstudio.microsoft.com/download/pr/ebdab8e5-1d7b-4d9f-a11b-cbb1720c3b12/843068991DAAA1F73AD9F6239BCE4D0F6A07A51F18C37EA2A867E9BECA71295C/VC_redist.x64.exe",
        "sha256": "843068991daaa1f73ad9f6239bce4d0f6a07a51f18c37ea2a867e9beca71295c",
        "bytes": 18731856,
        "checksumSource": "Microsoft content-addressed download URL, resolved from https://aka.ms/vc14/vc_redist.x64.exe",
    },
}

LICENSES = {
    "ytDlpLicense": {"filename": "yt-dlp-LICENSE.txt", "url": "https://raw.githubusercontent.com/yt-dlp/yt-dlp/2026.08.19/LICENSE", "sha256": "7e12e5df4bae12cb21581ba157ced20e1986a0508dd10d0e8a4ab9a4cf94e85c"},
    "ytDlpThirdParty": {"filename": "yt-dlp-THIRD_PARTY_LICENSES.txt", "url": "https://raw.githubusercontent.com/yt-dlp/yt-dlp/2026.08.19/THIRD_PARTY_LICENSES.txt", "sha256": "472aefe951c7db35e1657c1d13fd337140511ed6f2b329205105ad441c5a02b7"},
    "whisperLicense": {"filename": "Whisper-LICENSE.txt", "url": "https://raw.githubusercontent.com/ggml-org/whisper.cpp/v1.8.2/LICENSE", "sha256": "e562a2ddfaf8280537795ac5ecd34e3012b6582a147ef69ba6a6a5c08c84757d"},
    "visualCLicense": {"filename": "Microsoft-VC-Runtime-License.docx", "url": "https://visualstudio.microsoft.com/wp-content/uploads/2025/10/Visual-C-V14-License-Redistributable_and_Runtime_ENU.docx", "sha256": "08651651a7602fc7c0e2763de0fde1ff9f868df2780597cd1775ee9d6441c783"},
}

NOTICE = """Graffin Studio 2.4 — Windows x64

FFmpeg and FFprobe 9.0.2, static Windows x64 essentials build by Gyan.
GPL version 3. See FFmpeg-GPL-3.0.txt and FFmpeg-BUILD-README.txt, including
the exact build configuration and component licenses.
Binary distribution: https://github.com/GyanD/codexffmpeg/releases/tag/9.0.2
FFmpeg source: https://github.com/FFmpeg/FFmpeg/commit/946fcce07b
Build information and corresponding build scripts: https://www.gyan.dev/ffmpeg/builds/

whisper.cpp 1.8.2 CPU x64 — MIT; see media-tools/Whisper-LICENSE.txt.
Source: https://github.com/ggml-org/whisper.cpp/tree/v1.8.2
The Windows CPU binary requires AVX2, FMA and F16C; no CUDA/RTX is required.
The local ggml-small-q5_1 speech model is downloaded separately on first setup.

Node.js 24.21.0 Windows x64 — MIT and bundled third-party notices.
See media-tools/Node-LICENSE.txt. https://nodejs.org/dist/v24.21.0/

yt-dlp 2026.08.19 Windows x64. Source: Unlicense. The combined PyInstaller
binary includes GPLv3+ components. See FFmpeg-GPL-3.0.txt,
media-tools/yt-dlp-LICENSE.txt and media-tools/yt-dlp-THIRD_PARTY_LICENSES.txt.
Source and binary releases: https://github.com/yt-dlp/yt-dlp/tree/2026.08.19

Microsoft Visual C++ v14 runtime 14.51.36247.0: four unmodified app-local x64
DLLs from Microsoft's signed VC_redist.x64.exe. Microsoft license terms are
in media-tools/Microsoft-VC-Runtime-License.txt.
Original distribution: https://aka.ms/vc14/vc_redist.x64.exe
The runtime does not require system installation or administrator privileges.

Application framework: Tauri (MIT or Apache-2.0), https://github.com/tauri-apps/tauri
React (MIT), https://github.com/facebook/react
Lucide icons (ISC), https://github.com/lucide-icons/lucide
SHA-256 helpers: sha2 (MIT or Apache-2.0).
Additional dependency license texts: DEPENDENCY_LICENSES.txt.

Artwork and banner were supplied or created for the owner of this project.
Windows runtime versions, SHA-256 hashes, upstream URLs, file sizes and PE
import tables are in media-tools/windows-runtime-manifest.json.
"""


def digest(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as source:
        while data := source.read(1024 * 1024):
            result.update(data)
    return result.hexdigest()


def fetch(asset: dict, cache: Path) -> Path:
    target = cache / asset["filename"]
    if target.is_file() and digest(target) == asset["sha256"]:
        return target
    temporary = target.with_suffix(target.suffix + ".partial")
    request = urllib.request.Request(asset["url"], headers={"User-Agent": "Graffin-Studio-Windows-build/2.4.0"})
    try:
        with urllib.request.urlopen(request, timeout=90) as response, temporary.open("wb") as destination:
            copied = 0
            while data := response.read(1024 * 1024):
                copied += len(data)
                if copied > 150_000_000:
                    raise RuntimeError("Asset exceeds reviewed download limit")
                destination.write(data)
        if digest(temporary) != asset["sha256"]:
            raise RuntimeError(f"SHA-256 mismatch for {asset['filename']}")
        if "bytes" in asset and temporary.stat().st_size != asset["bytes"]:
            raise RuntimeError(f"File size mismatch for {asset['filename']}")
        temporary.replace(target)
    finally:
        temporary.unlink(missing_ok=True)
    return target


def member(archive: zipfile.ZipFile, suffix: str) -> str:
    choices = [name for name in archive.namelist() if name.endswith(suffix)]
    if len(choices) != 1:
        raise RuntimeError(f"Expected exactly one reviewed archive member: {suffix}; got {choices}")
    return choices[0]


def write_member(archive: zipfile.ZipFile, name: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(archive.read(name))


def pe_metadata(path: Path) -> dict:
    data = path.read_bytes()
    if data[:2] != b"MZ":
        raise RuntimeError(f"Not a Windows executable: {path.name}")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise RuntimeError(f"Bad PE signature: {path.name}")
    machine, count, timestamp, _, _, optional_size, _ = struct.unpack_from("<HHIIIHH", data, pe + 4)
    if machine != 0x8664:
        raise RuntimeError(f"Expected AMD64 executable: {path.name}: {machine:#x}")
    optional = pe + 24
    if struct.unpack_from("<H", data, optional)[0] != 0x20B:
        raise RuntimeError(f"Expected PE32+ executable: {path.name}")
    sections = []
    for index in range(count):
        position = optional + optional_size + index * 40
        virtual_size, rva, raw_size, raw = struct.unpack_from("<IIII", data, position + 8)
        sections.append((rva, max(virtual_size, raw_size), raw))

    def position(rva: int) -> int:
        for start, size, raw in sections:
            if start <= rva < start + size:
                return raw + rva - start
        raise RuntimeError(f"Unmapped PE RVA {rva:#x} in {path.name}")

    def string(rva: int) -> str:
        offset = position(rva)
        return data[offset:data.index(b"\0", offset)].decode("ascii")

    imports = []
    import_rva, _ = struct.unpack_from("<II", data, optional + 112 + 8)
    if import_rva:
        descriptor = position(import_rva)
        while any(data[descriptor:descriptor + 20]):
            imports.append(string(struct.unpack_from("<I", data, descriptor + 12)[0]))
            descriptor += 20
    return {"machine": "AMD64", "format": "PE32+", "imports": sorted(set(imports), key=str.casefold)}


def stage_visual_c_runtime(installer: Path, cache: Path, sevenzip: Path, tools: Path) -> None:
    """Extract only Microsoft's four app-local x64 DLLs, without executing setup."""
    if not sevenzip.is_file():
        raise RuntimeError("Provide --sevenzip pointing to a 7-Zip command-line executable")
    data = installer.read_bytes()
    cabinets = []
    start = 0
    while True:
        offset = data.find(b"MSCF", start)
        if offset < 0:
            break
        start = offset + 4
        length = struct.unpack_from("<I", data, offset + 8)[0]
        if 36 <= length <= len(data) - offset:
            cabinets.append((offset, length))
    if not cabinets:
        raise RuntimeError("Reviewed Visual C++ installer contains no CAB payload")
    offset, length = max(cabinets, key=lambda item: item[1])
    payload = cache / "vc-redist-payload.cab"
    payload.write_bytes(data[offset:offset + length])
    extracted = cache / "vc-redist-staging"
    extracted.mkdir(exist_ok=True)
    subprocess.run([str(sevenzip.resolve()), "x", "-y", "-o" + str(extracted.resolve()), str(payload.resolve())], check=True, capture_output=True)
    # a4 is the reviewed x64 Minimum Runtime CAB in this pinned Microsoft package.
    cab = extracted / "a4"
    dll_folder = extracted / "runtime"
    names = ["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "vcomp140.dll"]
    subprocess.run([str(sevenzip.resolve()), "x", "-y", "-o" + str(dll_folder.resolve()), str(cab.resolve()), *[name + "_amd64" for name in names]], check=True, capture_output=True)
    for name in names:
        source = dll_folder / (name + "_amd64")
        pe_metadata(source)
        shutil.copyfile(source, tools / name)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cache-dir", type=Path, default=Path(__file__).resolve().parent.parent / "windows-media-assets")
    parser.add_argument("--sevenzip", type=Path, help="7-Zip CLI used to extract Microsoft's app-local DLLs")
    args = parser.parse_args()
    args.cache_dir.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parent / "src-tauri/resources"
    tools = root / "media-tools"
    tools.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=4) as executor:
        downloads = dict(zip(ASSETS, executor.map(lambda asset: fetch(asset, args.cache_dir), ASSETS.values())))
    with ThreadPoolExecutor(max_workers=4) as executor:
        licenses = dict(zip(LICENSES, executor.map(lambda asset: fetch(asset, args.cache_dir), LICENSES.values())))
    with zipfile.ZipFile(downloads["ffmpeg"]) as archive:
        write_member(archive, member(archive, "/bin/ffmpeg.exe"), root / "ffmpeg.exe")
        write_member(archive, member(archive, "/bin/ffprobe.exe"), root / "ffprobe.exe")
        write_member(archive, member(archive, "/LICENSE"), root / "FFmpeg-GPL-3.0.txt")
        write_member(archive, member(archive, "/README.txt"), root / "FFmpeg-BUILD-README.txt")
    ffmpeg = (root / "ffmpeg.exe").read_bytes()
    for feature in [b"--enable-libass", b"--enable-libx264", b"--enable-gpl", b"--enable-static"]:
        if feature not in ffmpeg:
            raise RuntimeError(f"Required FFmpeg build flag missing: {feature.decode()}")
    with zipfile.ZipFile(downloads["node"]) as archive:
        write_member(archive, "node-v24.21.0-win-x64/node.exe", tools / "node.exe")
        write_member(archive, "node-v24.21.0-win-x64/LICENSE", tools / "Node-LICENSE.txt")
    with zipfile.ZipFile(downloads["whisper"]) as archive:
        write_member(archive, member(archive, "/whisper-cli.exe"), tools / "whisper-cli.exe")
        seen = set()
        for name in archive.namelist():
            if name.lower().endswith(".dll") and Path(name).name.casefold() != "sdl2.dll":
                destination = tools / Path(name).name
                if destination.name.casefold() in seen:
                    raise RuntimeError(f"Unexpected duplicate DLL: {destination.name}")
                seen.add(destination.name.casefold())
                write_member(archive, name, destination)
    shutil.copyfile(downloads["ytDlp"], tools / "yt-dlp.exe")
    stage_visual_c_runtime(downloads["visualCRuntime"], args.cache_dir, args.sevenzip or args.cache_dir / "7zip/7zz", tools)
    for name in ["ytDlpLicense", "ytDlpThirdParty", "whisperLicense"]:
        shutil.copyfile(licenses[name], tools / LICENSES[name]["filename"])
    with zipfile.ZipFile(licenses["visualCLicense"]) as archive:
        document = ET.fromstring(archive.read("word/document.xml"))
        paragraphs = document.findall(".//{http://schemas.openxmlformats.org/wordprocessingml/2006/main}p")
        text = "\n".join("".join(paragraph.itertext()) for paragraph in paragraphs)
        (tools / "Microsoft-VC-Runtime-License.txt").write_text("Source: " + LICENSES["visualCLicense"]["url"] + "\n\n" + text + "\n")
    # These Mach-O resources belong only to the separate macOS build.
    (root / "ffmpeg").unlink(missing_ok=True)
    (tools / "whisper-cli").unlink(missing_ok=True)
    (tools / "SDL2.dll").unlink(missing_ok=True)
    (tools / "runtime-manifest.json").unlink(missing_ok=True)
    (root / "THIRD_PARTY_NOTICES.txt").write_text(NOTICE)
    (root / "FFmpeg-LICENSE.txt").write_text("FFmpeg / FFprobe 9.0.2, Windows x64 static essentials build by Gyan.\nLicense: GPL version 3; see FFmpeg-GPL-3.0.txt.\nExact build configuration and source commit: FFmpeg-BUILD-README.txt.\nhttps://github.com/GyanD/codexffmpeg/releases/tag/9.0.2\n")
    metadata = {str(path.relative_to(root)): {"bytes": path.stat().st_size, "sha256": digest(path), **pe_metadata(path)} for path in sorted(root.rglob("*")) if path.is_file() and path.suffix.lower() in {".exe", ".dll"}}
    report = {"platform": "windows-x64", "assets": ASSETS, "licenses": LICENSES, "files": metadata, "speechBackend": "CPU; AVX2, FMA and F16C required; no CUDA or RTX requirement", "visualCRuntime": "Four unmodified app-local x64 DLLs from the Microsoft-signed redistributable; no system installation needed", "runtimeExecutionTested": False}
    (tools / "windows-runtime-manifest.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
