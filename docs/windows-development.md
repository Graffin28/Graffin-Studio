# Graffin Studio 2.4 — Windows x64

Windows-порт монтажной версии Graffin Studio 2.4. Оригинальная установленная Mac-версия не изменяется этой сборкой.

Для передачи пользователю предназначен ZIP с установщиком EXE и инструкцией. Установщик включает FFmpeg/FFprobe, Node.js, yt-dlp, Whisper CPU, необходимые DLL, шрифты и баннер. Python и средства разработки пользователю не нужны. При необходимости установщик подготавливает WebView2 через встроенный bootstrapper; интернет требуется для его установки и первоначальной загрузки модели речи (около 190 МБ).

Система: Windows 10 1803+ / Windows 11 x64. Для автоматических субтитров и речевого подбора моментов нужны AVX2, FMA и F16C. На старом процессоре остаётся монтаж по времени без ASR. Видеокарта RTX для CPU-распознавания не требуется.

Сохранены монтаж, исходники из файлов/папок/ссылок, отдельные папки загрузок и экспорта, проверка и уточнение моментов перед сборкой, группы субтитров по 1–5 слов, анимации и подсветка, собственные шрифты, музыка, баннер-пауза и пресеты. Очереди и публикаций в аккаунты нет.

## Проверки

- `qa/windows-ui-results.json`: 7 сценариев интерфейса с Windows-путями на Chromium/macOS с синтетическим IPC.
- `src-tauri/resources/media-tools/windows-media-validation.json`: PE AMD64, зависимости и мультимедийные ресурсы.
- `qa/windows-installer-validation.json`: содержимое распакованного NSIS и совпадение файлов с исходной сборкой; создаётся после упаковки.
- `qa/windows-package.json`: целостность конечного ZIP; создаётся после упаковки.

Сборка выполняется на macOS для Windows через cargo-xwin/LLVM/NSIS. Фактический запуск Windows GUI и обработка MP4 на Windows в этой среде не проверены. Установщик не подписан сертификатом издателя.

## Повторная сборка

`prepare_windows_resources.py` подготавливает фиксированные Windows-ресурсы и проверяет SHA-256; извлечение Microsoft app-local DLL использует официальный 7-Zip. На Windows разработчику нужны Node.js, Rust, MSVC Build Tools и Windows SDK. `tauri.windows.conf.json` включает рекурсивное копирование ресурсов и NSIS.

```sh
npm ci
python prepare_windows_resources.py --help
npm run tauri -- build --target x86_64-pc-windows-msvc
python package_windows.py --installer /path/to/setup.exe --executable /path/to/barsik-studio.exe --archive /path/to/Graffin_Studio_2.4.0_Windows_x64.zip
```

При cross-компиляции используется `--runner cargo-xwin`. Не добавлять `+crt-static`: стандартный Tauri static-VCRuntime уже использует системную UCRT, а принудительный режим cargo-xwin создаёт конфликт линковки.
