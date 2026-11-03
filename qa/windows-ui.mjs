// Synthetic IPC on macOS Chromium: verifies Windows path handling in the UI.
// It does not launch the Windows executable or touch live user media/settings.
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {chromium} from '/Users/administrator/Documents/Codex/2026-10-23/new-chat/work/qa-deps/playwright-core/index.mjs';
import {installStudioFixture} from '/Users/administrator/Documents/Codex/2026-10-17/3-2-barsik-vpn-9-16/work/barsik-studio/qa/studio22-ui/fixture.mjs';
import {installGraffin24Fixture} from '/Users/administrator/Documents/Codex/2026-10-17/3-2-barsik-vpn-9-16/work/barsik-studio/qa/graffin24-ui/fixture.mjs';
function installWindowsFixture(){
 const f=window.__studioFixture,copy=value=>structuredClone(value),prior=window.__TAURI_INTERNALS__.invoke;
 f.windows={output:String.raw`C:\Users\Друг\Videos\Готовые ролики`,downloads:String.raw`C:\Users\Друг\Downloads\Видео по ссылкам`,top:String.raw`D:\Нарезки\Исходники`,bottom:String.raw`D:\Нарезки\Фон`};
 Object.assign(f.settings,{outputDir:f.windows.output,importDir:f.windows.downloads,topSourceDir:'',bottomSourceDir:'',cutMode:'fixed',count:2,duration:12});
 f.folderTop={...f.folderTop,path:f.windows.top+String.raw`\Интервью.mp4`,name:'Интервью.mp4'};
 f.folderBottom={...f.folderBottom,path:f.windows.bottom+String.raw`\Фоновое видео.mp4`,name:'Фоновое видео.mp4'};
 f.outputFile=f.windows.output+String.raw`\Graffin_001.mp4`;
 window.__TAURI_INTERNALS__.convertFileSrc=path=>path.startsWith('/')?path:'/fixture/windows/'+encodeURIComponent(path);
 window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
  const record=()=>f.calls.push({command,args:copy(args)});
  if(command==='bootstrap'){record();return {defaultOutput:f.windows.output,defaultImportDir:f.windows.downloads,bannerPath:'/banner.png',bannerPreview:'/banner.png',version:'2.4.0',ffmpegReady:true,restoredSettings:copy(f.settings)};}
  if(command==='plugin:dialog|open'){
   const title=args.options?.title||'';
   const chosen={'Папка основных видео':f.windows.top,'Папка фоновых видео':f.windows.bottom,'Куда сохранять видео по ссылкам':f.windows.downloads,'Куда сохранять готовые ролики':f.windows.output,'Куда сохранить готовые ролики':f.windows.output}[title];
   if(chosen){record();return chosen;}
  }
  if(command==='inspect_media_folder'){record();return[copy(args.path===f.windows.bottom?f.folderBottom:f.folderTop)];}
  if(command==='import_media_url'){record();return{...copy(f.folderTop),id:'windows-linked',path:f.windows.downloads+String.raw`\Импорт-test\По ссылке.mp4`,name:'По ссылке.mp4'};}
  if(command==='start_batch'){record();f.job={id:'windows-job',status:'completed',total:2,completed:2,current:2,progress:1,message:'Готово',outputs:[f.outputFile],error:null};return copy(f.job);}
  return prior(command,args);
 };
}
const browser=await chromium.launch({executablePath:'/Applications/Brave Browser.app/Contents/MacOS/Brave Browser',headless:true});
const results={environment:'macOS headless Chromium, synthetic Tauri IPC',windowsExecutableLaunched:false,checks:[],errors:[],liveUserFilesOrSettingsTouched:false};
let page;
try{
 page=await browser.newPage({viewport:{width:1440,height:900},reducedMotion:'reduce'});page.setDefaultTimeout(12000);
 page.on('pageerror',error=>results.errors.push(String(error)));
 const original='/Users/administrator/Documents/Codex/2026-10-17/3-2-barsik-vpn-9-16/work/barsik-studio';
 await page.route('**/qa/graffin24-design/source-*.jpg',async route=>route.fulfill({status:200,contentType:'image/jpeg',body:await readFile(original+'/qa/graffin24-design/'+(route.request().url().includes('background')?'source-background.jpg':'source-main.jpg'))}));
 await page.route('**/fixture/windows/**',async route=>route.fulfill({status:200,contentType:'video/mp4',body:await readFile(original+'/qa/graffin24-ui/raw-preview.mp4')}));
 await page.addInitScript({content:`(${installStudioFixture.toString()})();(${installGraffin24Fixture.toString()})();(${installWindowsFixture.toString()})();`});
 await page.goto('http://127.0.0.1:1429/');await page.waitForLoadState('networkidle');
 const paths=await page.evaluate(()=>window.__studioFixture.windows);
 assert.equal(await page.locator('.header-output').innerText(),'Готовые ролики');
 assert.equal(await page.locator('.header-output').getAttribute('title'),paths.output);
 assert.equal(await page.getByRole('button',{name:'Выбрать папку: Видео по ссылкам',exact:true}).innerText(),paths.downloads);
 assert.equal(await page.getByRole('button',{name:'Выбрать папку: Готовые ролики',exact:true}).innerText(),paths.output);
 results.checks.push('Header uses the final Windows folder name; complete Unicode drive paths remain visible for download/output locations');
 await page.getByRole('button',{name:'Основные видео из папки',exact:true}).click();
 await page.getByRole('button',{name:'Показать Интервью.mp4',exact:true}).waitFor();
 assert.equal(await page.getByLabel('Верхние видео',{exact:true}).locator('.source-folder-path').innerText(),'Последняя папка: Исходники');
 assert.equal(await page.getByLabel('Верхние видео',{exact:true}).locator('.source-folder-path').getAttribute('title'),paths.top);
 await page.getByRole('button',{name:'Фоновые видео из папки',exact:true}).click();
 await page.getByRole('button',{name:'Показать Фоновое видео.mp4',exact:true}).waitFor();
 assert.equal(await page.getByLabel('Нижние видео',{exact:true}).locator('.source-folder-path').innerText(),'Последняя папка: Фон');
 const scans=await page.evaluate(()=>window.__studioFixture.calls.filter(x=>x.command==='inspect_media_folder').map(x=>x.args.path));
 assert.deepEqual(scans,[paths.top,paths.bottom]);
 results.checks.push('Source folders on D: are sent intact to native inspection and display correct short names');
 for(const [label,key] of [['Видео по ссылкам','downloads'],['Готовые ролики','output']]){
  await page.getByRole('button',{name:'Выбрать папку: '+label,exact:true}).click();
  await page.getByRole('button',{name:'Открыть папку: '+label,exact:true}).click();
  assert.equal(await page.evaluate(()=>window.__studioFixture.calls.filter(x=>x.command==='open_folder').at(-1).args.path),paths[key]);
 }
 results.checks.push('Folder selection and open_folder preserve exact Windows paths including Cyrillic characters and spaces');
 await page.getByRole('button',{name:'Основное видео по ссылке',exact:true}).click();
 const importer=page.getByRole('dialog',{name:'Основное видео по ссылке',exact:true});await importer.waitFor();
 assert.ok((await importer.innerText()).includes(paths.downloads));
 await page.getByRole('textbox',{name:'Ссылка на видео',exact:true}).fill('https://example.test/public.mp4');
 await importer.getByRole('button',{name:'Загрузить',exact:true}).click();await importer.waitFor({state:'hidden'});
 assert.equal(await page.evaluate(()=>window.__studioFixture.calls.find(x=>x.command==='import_media_url').args.importDir),paths.downloads);
 results.checks.push('URL dialog shows the complete destination and sends importDir without changing Windows separators');
 await page.getByRole('button',{name:'Основные видео из папки',exact:true}).click();
 await page.waitForFunction(()=>window.__studioFixture.settings.topSourceDir===window.__studioFixture.windows.top);
 await page.locator('.export-actions').getByRole('button',{name:'Собрать 2 ролика',exact:true}).click();
 await page.locator('.export-actions').getByRole('button',{name:'Готовые ролики',exact:true}).waitFor();
 const exported=await page.evaluate(()=>window.__studioFixture.calls.find(x=>x.command==='start_batch').args.request);
 assert.equal(exported.settings.outputDir,paths.output);assert.equal(exported.settings.importDir,paths.downloads);
 assert.equal(exported.top[0].path,paths.top+String.raw`\Интервью.mp4`);assert.equal(exported.bottom[0].path,paths.bottom+String.raw`\Фоновое видео.mp4`);
 await page.locator('.export-actions').getByRole('button',{name:'Готовые ролики',exact:true}).click();
 assert.equal(await page.evaluate(()=>window.__studioFixture.calls.filter(x=>x.command==='open_folder').at(-1).args.path),paths.output+String.raw`\Graffin_001.mp4`);
 results.checks.push('Export receives Windows source/destination paths; opening the result passes its full native file path');
 await page.setViewportSize({width:1000,height:720});
 const tabs=page.getByRole('tablist',{name:'Настройки ролика'});
 for(const tab of ['Кадр','Звук','Текст','Нарезка']){
  await tabs.getByRole('tab',{name:tab,exact:true}).click();
  const geometry=await page.evaluate(()=>{const frame=document.querySelector('.composition-frame').getBoundingClientRect(),timeline=document.querySelector('.studio-timeline').getBoundingClientRect(),footer=document.querySelector('.export-bar').getBoundingClientRect();return{width:document.documentElement.scrollWidth,height:document.documentElement.scrollHeight,vw:innerWidth,vh:innerHeight,frame:frame.toJSON(),timeline:timeline.toJSON(),footer:footer.toJSON()};});
  assert.equal(geometry.width,geometry.vw);assert.equal(geometry.height,geometry.vh);assert.ok(geometry.frame.height>180);assert.ok(geometry.frame.bottom<=geometry.timeline.top+1);assert.ok(geometry.timeline.bottom<=geometry.footer.top+1);
 }
 await tabs.getByRole('tab',{name:'Текст',exact:true}).click();await page.screenshot({path:'qa/windows-ui-1000x720.png'});
 results.checks.push('All four inspector tabs fit 1000×720 with Windows paths and no document overflow or footer overlap');
 assert.equal(await page.getByRole('button',{name:'Очередь',exact:true}).count(),0);assert.equal(await page.getByRole('button',{name:'Публикация',exact:true}).count(),0);
 const social=await page.evaluate(()=>window.__studioFixture.calls.filter(x=>/^(queue_|publication_|account_|browser_connection)/.test(x.command)));assert.deepEqual(social,[]);assert.deepEqual(results.errors,[]);
 results.checks.push('No queue, account, publication or browser-session commands are used');
 results.passed=true;
}catch(error){results.passed=false;results.failure=String(error);console.error(error);if(page&&!page.isClosed())await page.screenshot({path:'qa/windows-ui-failure.png'});process.exitCode=1;}
finally{await writeFile('qa/windows-ui-results.json',JSON.stringify(results,null,2)+'\n');await browser.close();}
