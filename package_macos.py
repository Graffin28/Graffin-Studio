"""Package a release binary without inherited iCloud/Finder metadata.

Run the Tauri release build first. The template is a Tauri-generated .app.
The temporary, clean bundle is signed before it is archived into outputs/.
"""
import argparse
import json
import os
import plistlib
from pathlib import Path
import shutil
import subprocess
import tempfile


def copy_clean(source: Path, target: Path):
    for folder, directories, files in os.walk(source):
        destination = target / Path(folder).relative_to(source)
        destination.mkdir(parents=True, exist_ok=True)
        for name in directories:
            (destination / name).mkdir(exist_ok=True)
        for name in files:
            item = Path(folder) / name
            output = destination / name
            if item.is_symlink():
                output.symlink_to(os.readlink(item))
            else:
                shutil.copyfile(item, output)
                output.chmod(item.stat().st_mode & 0o777)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--template", required=True, type=Path)
    parser.add_argument("--archive", required=True, type=Path)
    args = parser.parse_args()
    project = Path(__file__).resolve().parent
    temporary = Path(tempfile.mkdtemp(prefix="barsik-studio-release-"))
    app = temporary / "Graffin Studio.app"
    copy_clean(args.template, app)
    resources = app / "Contents/Resources/resources"
    if resources.exists():
        shutil.rmtree(resources)
    copy_clean(project / "src-tauri/resources", resources)
    executable = app / "Contents/MacOS/barsik-studio"
    shutil.copyfile(project / "src-tauri/target/release/barsik-studio", executable)
    executable.chmod(0o755)
    # The template can be an older Tauri bundle; all runtime code comes from
    # the newly built executable, and Finder must show its actual version.
    info_path = app / "Contents/Info.plist"
    info = plistlib.loads(info_path.read_bytes())
    config = json.loads((project / "src-tauri/tauri.conf.json").read_text())
    version = config["version"]
    info["CFBundleShortVersionString"] = version
    info["CFBundleVersion"] = version
    info["CFBundleName"] = config["productName"]
    info["CFBundleDisplayName"] = config["productName"]
    info["CFBundleIconFile"] = "icon.icns"
    info["LSMinimumSystemVersion"] = config["bundle"]["macOS"]["minimumSystemVersion"]
    info_path.write_bytes(plistlib.dumps(info))
    shutil.copyfile(project / "src-tauri/icons/icon.icns", app / "Contents/Resources/icon.icns")
    for executable_resource in [resources / "media-tools/whisper-cli"]:
        if executable_resource.exists():
            subprocess.run(["codesign", "--force", "--sign", "-", str(executable_resource)], check=True)
    subprocess.run(["codesign", "--force", "--deep", "--sign", "-", str(app)], check=True)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)], check=True)
    args.archive.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["ditto", "-c", "-k", "--keepParent", "--norsrc", str(app), str(args.archive)], check=True)
    result = {
        "app": str(app), "archive": str(args.archive.resolve()),
        "archiveBytes": args.archive.stat().st_size,
        "appBytes": sum(p.stat().st_size for p in app.rglob("*") if p.is_file()),
        "signing": "ad-hoc", "notarized": False, "architecture": "arm64",
        "version": version,
    }
    (project / "qa/final-bundle.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
