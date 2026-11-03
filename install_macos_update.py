"""Install the already-signed release atomically, preserving the prior app and config."""
from pathlib import Path
from datetime import datetime,timezone
import hashlib,json,os,plistlib,shutil,subprocess
from package_macos import copy_clean

root=Path(__file__).resolve().parent
release=json.loads((root/'qa/final-bundle.json').read_text())
source=Path(release['app'])
if not source.is_dir():raise SystemExit('Packaged app is missing')
version=plistlib.loads((source/'Contents/Info.plist').read_bytes())['CFBundleShortVersionString']
expected=json.loads((root/'src-tauri/tauri.conf.json').read_text())['version']
if version!=expected:raise SystemExit('Packaged version does not match the project release')
# File inspection, not UI automation. Existing app must first be quit via its UI.
if subprocess.run(['pgrep','-x','barsik-studio'],stdout=subprocess.DEVNULL).returncode==0:raise SystemExit('Quit the existing Graffin Studio app first')
subprocess.run(['codesign','--verify','--deep','--strict',str(source)],check=True)
stamp=datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
backup=root/'qa/app-update-backup'/stamp
backup.mkdir(parents=True,exist_ok=False)
config=Path.home()/'Library/Application Support/com.barsik.studio'
configuration_names=['settings.json','studio-presets-v2.json','render-history-v1.json']
original={name:hashlib.sha256((config/name).read_bytes()).hexdigest() for name in configuration_names if (config/name).is_file()}
(backup/'app-config').mkdir()
for name in original:shutil.copyfile(config/name,backup/'app-config'/name)
# Browser profiles and session handoff files stay in place; never copy credentials into QA.
installed=Path('/Applications/Graffin Studio.app')
stage=installed.with_name('.Graffin Studio.installing.app')
if stage.exists():raise SystemExit('Staging app already exists; inspect it before retrying')
copy_clean(source,stage)
subprocess.run(['codesign','--verify','--deep','--strict',str(stage)],check=True)
previous=[]
for existing in [installed,Path('/Applications/Barsik Studio.app')]:
 if existing.exists():
  destination=backup/existing.name
  existing.rename(destination)
  previous.append((existing,destination))
try:stage.rename(installed)
except BaseException:
 for original_path,saved_path in previous:
  if saved_path.exists():saved_path.rename(original_path)
 raise
subprocess.run(['/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister','-f',str(installed)],check=True)
result={'installedApp':str(installed),'version':version,'backup':str(backup),'configBefore':original,'configPreserved':all((config/name).exists() and hashlib.sha256((config/name).read_bytes()).hexdigest()==digest for name,digest in original.items()),'launched':False}
(root/'qa/installed-update.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
