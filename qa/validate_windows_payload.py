"""Validate unpacked NSIS payload without installing or executing Windows code."""
from pathlib import Path
import argparse, hashlib, json, struct, sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from prepare_windows_resources import pe_metadata

def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as file:
        for block in iter(lambda:file.read(1024*1024),b''):h.update(block)
    return h.hexdigest()

parser=argparse.ArgumentParser()
parser.add_argument('payload',type=Path)
parser.add_argument('built_executable',type=Path)
args=parser.parse_args()
root=Path(__file__).resolve().parents[1]
apps=list(args.payload.rglob('barsik-studio.exe'))
assert len(apps)==1,apps
app=apps[0]
built=args.built_executable.read_bytes();packaged=app.read_bytes()
marker=b'__TAURI_BUNDLE_TYPE_VAR_UNK'
# Tauri stamps NSS into the installer payload and then restores UNK in target/.
expected=built.replace(marker,b'__TAURI_BUNDLE_TYPE_VAR_NSS',1) if built.count(marker)==1 else built
assert packaged==expected or packaged==built,'Packaged executable differs beyond the documented NSIS bundle marker'
metadata=pe_metadata(app)
assert not any('vcruntime' in name.lower() or 'msvcp' in name.lower() for name in metadata['imports'])
main_system={'advapi32.dll','bcryptprimitives.dll','combase.dll','comctl32.dll','dwmapi.dll','gdi32.dll','imm32.dll','kernel32.dll','ntdll.dll','ole32.dll','oleaut32.dll','shell32.dll','shlwapi.dll','user32.dll','ws2_32.dll'}
assert all(name.lower() in main_system or name.lower().startswith(('api-ms-win-','ext-ms-win-')) for name in metadata['imports']),metadata
checked=[]
for source in sorted((root/'src-tauri/resources').rglob('*')):
    if not source.is_file():continue
    relative=source.relative_to(root/'src-tauri/resources')
    destination=app.parent/'resources'/relative
    assert destination.is_file(),f'Missing packaged resource: {relative}'
    assert digest(source)==digest(destination),f'Changed packaged resource: {relative}'
    with destination.open('rb') as file: magic=file.read(4)
    assert magic not in [b'\xcf\xfa\xed\xfe',b'\xfe\xed\xfa\xcf',b'\xca\xfe\xba\xbe'],f'Mac executable packaged: {relative}'
    checked.append(str(relative))
for key in ['fonts','media-tools']:
    assert any(name.startswith(key+'/') for name in checked)
bootstrap=[p for p in args.payload.rglob('*.exe') if 'webview' in p.name.lower()]
assert bootstrap,'Missing WebView2 bootstrapper'
data=app.read_bytes();pos=data.find(struct.pack('<I',0xFEEF04BD));assert pos>=0,'Missing Windows version resource'
_,_,ms,ls=struct.unpack_from('<IIII',data,pos)
version=[ms>>16,ms&65535,ls>>16,ls&65535]
assert version==[2,4,0,0],version
result={'application':metadata,'fileVersion':version,'packagedApplicationMatchesBuildExceptNsisBundleMarker':True,'resourcesVerified':len(checked),'allResourceHashesMatch':True,'fontsAndMediaToolsPresent':True,'webView2BootstrapperPresent':True,'macExecutablesAbsent':True,'windowsExecutionTested':False}
(root/'qa/windows-installer-validation.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(result,ensure_ascii=False,indent=2))
