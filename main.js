const {app,BrowserWindow,ipcMain,screen,shell,Tray,Menu,nativeImage,dialog,powerMonitor}=require('electron');
const os=require('node:os'),fs=require('node:fs'),path=require('node:path'),{execFile}=require('node:child_process');
let win,tray,previous,settings={shortcuts:[],hide:false},quitting=false;
const defaults=[{name:'Explorer',target:'explorer.exe'},{name:'Browser',target:'https://www.google.com'},{name:'Settings',target:'ms-settings:'}];
function save(){fs.writeFileSync(path.join(app.getPath('userData'),'settings.json'),JSON.stringify(settings));}
function position(){const d=screen.getPrimaryDisplay().workArea;win.setBounds({width:Math.min(900,d.width),height:100,x:Math.round(d.x+(d.width-Math.min(900,d.width))/2),y:d.y+d.height-100});}
function taskbar(hide){if(process.platform!=='win32')return;const script="$src = @'\nusing System; using System.Runtime.InteropServices; public class NB { [DllImport(\"user32.dll\")] public static extern IntPtr FindWindow(string c,string n); [DllImport(\"user32.dll\")] public static extern bool ShowWindow(IntPtr h,int n); }\n'@\nAdd-Type $src; [NB]::ShowWindow([NB]::FindWindow('Shell_TrayWnd',$null),"+(hide?0:5)+")";execFile('powershell.exe',['-NoProfile','-NonInteractive','-Command',script],{windowsHide:true},()=>{});}
function launch(target){if(target==='explorer.exe')execFile('explorer.exe',[],()=>{});else if(/^(https:\/\/|ms-settings:)/.test(target))shell.openExternal(target);else shell.openPath(target);}
app.whenReady().then(()=>{try{settings=JSON.parse(fs.readFileSync(path.join(app.getPath('userData'),'settings.json'),'utf8'));}catch{settings.shortcuts=defaults;} settings.hide=false;
win=new BrowserWindow({width:900,height:100,frame:false,transparent:true,alwaysOnTop:true,skipTaskbar:true,resizable:false,webPreferences:{preload:path.join(__dirname,'preload.js'),contextIsolation:true,nodeIntegration:false,sandbox:true}});win.loadFile('index.html');position();screen.on('display-metrics-changed',position);
const svg='<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" rx="8" fill="#252936"/><path d="M8 23V9l16 14V9" stroke="white" stroke-width="3" fill="none"/></svg>';tray=new Tray(nativeImage.createFromDataURL('data:image/svg+xml;base64,'+Buffer.from(svg).toString('base64')));tray.setToolTip('NovaBar');tray.setContextMenu(Menu.buildFromTemplate([{label:'Restore Windows taskbar',click:()=>{taskbar(false);settings.hide=false;win.webContents.send('changed');}},{label:'Quit NovaBar',click:()=>app.quit()}]));tray.on('click',()=>win.show());
win.on('close',()=>taskbar(false));});
app.on('before-quit',()=>{quitting=true;taskbar(false);});app.on('window-all-closed',()=>app.quit());
ipcMain.handle('state',()=>settings);
ipcMain.handle('stats',()=>{const totals=os.cpus().reduce((a,c)=>{a.idle+=c.times.idle;a.total+=Object.values(c.times).reduce((s,v)=>s+v,0);return a;},{idle:0,total:0});const cpu=previous?Math.round(100*(1-(totals.idle-previous.idle)/Math.max(1,totals.total-previous.total))):0;previous=totals;return {cpu,ram:Math.round(100*(1-os.freemem()/os.totalmem())),battery:powerMonitor.isOnBatteryPower()};});
ipcMain.handle('launch',(_,i)=>{const item=settings.shortcuts[i];if(item)launch(item.target);});
ipcMain.handle('add',async()=>{const r=await dialog.showOpenDialog(win,{properties:['openFile'],filters:[{name:'Applications',extensions:['exe','lnk']}]});if(!r.canceled){const target=r.filePaths[0];settings.shortcuts.push({name:path.basename(target,path.extname(target)),target});save();}return settings;});
ipcMain.handle('remove',(_,i)=>{if(Number.isInteger(i)&&i>=0)settings.shortcuts.splice(i,1);save();return settings;});
ipcMain.handle('hide',(_,v)=>{settings.hide=!!v;taskbar(settings.hide);return settings;});
ipcMain.handle('quit',()=>app.quit());
