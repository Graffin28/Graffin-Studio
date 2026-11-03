"""Package the verified Windows NSIS installer for direct sharing."""
from __future__ import annotations
import argparse, hashlib, json, struct, zipfile
from pathlib import Path


def pe_machine(path: Path) -> int:
    data = path.read_bytes()
    if data[:2] != b'MZ':
        raise ValueError(f'Not a Windows executable: {path.name}')
    offset = struct.unpack_from('<I', data, 0x3c)[0]
    if data[offset:offset + 4] != b'PE\0\0':
        raise ValueError(f'Invalid PE header: {path.name}')
    return struct.unpack_from('<H', data, offset + 4)[0]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--installer', required=True, type=Path)
    parser.add_argument('--executable', required=True, type=Path)
    parser.add_argument('--archive', required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    version = json.loads((root / 'src-tauri/tauri.conf.json').read_text())['version']
    assert pe_machine(args.executable) == 0x8664, 'Expected Windows x64 application'
    assert pe_machine(args.installer) in (0x14c, 0x8664), 'Expected Windows NSIS installer'
    setup_name = f'Graffin_Studio_{version}_Windows_x64_setup.exe'
    folder = f'Graffin Studio {version} Windows x64'
    readme = f'''GRAFFIN STUDIO {version} — WINDOWS x64

1. Распакуйте ZIP целиком в обычную папку.
2. Запустите {setup_name} и установите программу.
3. Откройте Graffin Studio через меню «Пуск».
4. Добавьте исходники и выберите папки для загрузок по ссылкам и готовых роликов.

Система: Windows 10 (1803 или новее) / Windows 11, 64-битная x64.
Windows ARM64 отдельно не проверялась.
Для автосубтитров и поиска моментов нужен процессор с AVX2, FMA и F16C.
Если процессор их не поддерживает, доступен монтаж по времени без ASR.

В установщике уже есть FFmpeg, распознавание речи Whisper CPU, загрузчик
видео, Node.js, шрифты и баннер. Python, Rust, Node.js и FFmpeg отдельно
устанавливать не нужно. Видеокарта RTX не требуется для CPU-распознавания.
Скорость обработки зависит от процессора, длительности и разрешения видео.

При необходимости установщик подготовит Microsoft Edge WebView2;
для этого нужен интернет. При первом использовании субтитров/поиска моментов
нажмите «Подготовить»: программа скачает модель речи (около 190 МБ).
Затем распознавание работает локально. Загрузка видео по ссылкам требует
интернета; доступны общедоступные ролики поддерживаемых сайтов.

В этой версии есть монтаж, выбор речевых моментов перед сборкой,
субтитры по 1–5 слов с анимациями и подсветкой, собственные шрифты,
музыка, баннер-пауза и пресеты. Разделы очереди и публикации в TikTok удалены.

Сохраняйте исходники и скрытую папку .graffin-edit рядом с готовыми видео,
если планируете впоследствии редактировать слова и пересобирать ролик.
Личные настройки и модель хранятся в профиле пользователя Windows,
они не входят в этот ZIP и не передаются вместе с архивом.

Установщик не подписан сертификатом издателя.
Сборка выполнена на macOS для Windows; фактический запуск интерфейса
и рендера на Windows в этой среде проверить не удалось.
'''
    checksum = f'{sha256(args.installer)}  {setup_name}\r\n'
    args.archive.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(args.archive, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        archive.write(args.installer, f'{folder}/{setup_name}')
        archive.writestr(f'{folder}/ПРОЧТИ МЕНЯ.txt', b'\xef\xbb\xbf' + readme.replace('\n', '\r\n').encode())
        archive.writestr(f'{folder}/SHA256.txt', checksum.encode())
    with zipfile.ZipFile(args.archive) as archive:
        assert archive.testzip() is None
        assert len(archive.namelist()) == 3
        assert archive.read(f'{folder}/{setup_name}') == args.installer.read_bytes()
    result = {'version': version, 'platform': 'Windows', 'architecture': 'x86_64',
              'archive': str(args.archive.resolve()), 'archiveBytes': args.archive.stat().st_size,
              'archiveSha256': sha256(args.archive), 'installerSha256': sha256(args.installer),
              'applicationMachine': 'AMD64', 'zipIntegrityVerified': True,
              'windowsRuntimeTested': False, 'signed': False}
    (root / 'qa/windows-package.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
