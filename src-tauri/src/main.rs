#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{sync::{Mutex,Arc,atomic::{AtomicBool,Ordering}}, path::PathBuf, process::Command, os::windows::process::CommandExt};
use serde::{Serialize,Deserialize};
use tauri::{Manager,State,AppHandle,PhysicalPosition,PhysicalSize};
use sysinfo::System;
#[derive(Clone,Serialize,Deserialize)]
struct Shortcut { name:String,target:String }
#[derive(Clone,Serialize,Deserialize)]
struct Settings { shortcuts:Vec<Shortcut>,hide:bool, #[serde(default)] appearance:Appearance }
#[derive(Clone,Serialize,Deserialize)] struct Appearance { width:u32, opacity:u8, accent:String, blur:bool }
impl Default for Appearance {fn default()->Self{Self{width:1100,opacity:72,accent:"#9c9cff".into(),blur:true}}}
struct Data { settings:Mutex<Settings>,system:Mutex<System>,last_foreground:Mutex<usize>,panel_open:Arc<AtomicBool> }
fn defaults()->Settings { Settings{hide:false,appearance:Appearance::default(),shortcuts:vec![Shortcut{name:"Explorer".into(),target:"explorer.exe".into()},Shortcut{name:"Browser".into(),target:"https://www.google.com".into()},Shortcut{name:"Settings".into(),target:"ms-settings:".into()}]} }
fn config(app:&AppHandle)->PathBuf { app.path().app_config_dir().unwrap().join("settings.json") }
fn save(app:&AppHandle,s:&Settings){let p=config(app);if let Some(d)=p.parent(){let _=std::fs::create_dir_all(d);}let _=std::fs::write(p,serde_json::to_string(s).unwrap());}
fn taskbar(hide:bool){unsafe{use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW,ShowWindow,SW_HIDE,SW_SHOW};let name:Vec<u16>="Shell_TrayWnd".encode_utf16().chain(Some(0)).collect();let h=FindWindowW(name.as_ptr(),std::ptr::null());if !h.is_null(){ShowWindow(h,if hide{SW_HIDE}else{SW_SHOW});}let secondary:Vec<u16>="Shell_SecondaryTrayWnd".encode_utf16().chain(Some(0)).collect();let mut after=std::ptr::null_mut();loop{let found=windows_sys::Win32::UI::WindowsAndMessaging::FindWindowExW(std::ptr::null_mut(),after,secondary.as_ptr(),std::ptr::null());if found.is_null(){break;}ShowWindow(found,if hide{SW_HIDE}else{SW_SHOW});after=found;}}}
#[tauri::command] fn state(data:State<Data>)->Settings{data.settings.lock().unwrap().clone()}
#[tauri::command] fn stats(data:State<Data>)->serde_json::Value{let mut s=data.system.lock().unwrap();s.refresh_cpu_usage();s.refresh_memory();let ram=100*s.used_memory()/s.total_memory().max(1);let mut power:windows_sys::Win32::System::Power::SYSTEM_POWER_STATUS=unsafe{std::mem::zeroed()};let ok=unsafe{windows_sys::Win32::System::Power::GetSystemPowerStatus(&mut power)};serde_json::json!({"cpu":s.global_cpu_usage().round() as u32,"ram":ram,"battery":if ok!=0 && power.BatteryLifePercent<=100 {Some(power.BatteryLifePercent)}else{None},"ac":ok!=0 && power.ACLineStatus==1})}
#[tauri::command] fn launch(i:usize,data:State<Data>)->Result<(),String>{let target={let s=data.settings.lock().unwrap();s.shortcuts.get(i).ok_or("Unknown shortcut")?.target.clone()};let last=*data.last_foreground.lock().unwrap();let wins=windows_list();let matching:Vec<_>=wins.iter().filter(|w|matches_target(&target,&w.path)).collect();if !matching.is_empty(){let chosen=matching.iter().find(|w|w.id.parse::<usize>().ok()==Some(last)).unwrap_or(&matching[0]);return activate_window_with_previous(chosen.id.clone(),last);}let target=&target;if target.starts_with("http://")||target.starts_with("https://")||target.starts_with("ms-settings:")||target.ends_with(".lnk"){Command::new("explorer.exe").arg(target).spawn().map_err(|e|e.to_string())?;}else{Command::new(target).spawn().map_err(|e|e.to_string())?;}Ok(())}
#[tauri::command] async fn add(app:AppHandle,data:State<'_,Data>)->Result<Settings,String>{let selected=tauri::async_runtime::spawn_blocking(||rfd::FileDialog::new().add_filter("Applications",&["exe","lnk"]).pick_file()).await.map_err(|e|e.to_string())?;let mut s=data.settings.lock().unwrap();if let Some(p)=selected{let name=p.file_stem().unwrap_or_default().to_string_lossy().into_owned();s.shortcuts.push(Shortcut{name,target:p.to_string_lossy().into_owned()});save(&app,&s);}Ok(s.clone())}
#[tauri::command] fn remove(i:usize,app:AppHandle,data:State<Data>)->Settings{let mut s=data.settings.lock().unwrap();if i<s.shortcuts.len(){s.shortcuts.remove(i);save(&app,&s);}s.clone()}
#[tauri::command] fn hide(v:bool,app:AppHandle,data:State<Data>)->Settings{let mut s=data.settings.lock().unwrap();if v && !s.hide{if let Ok(exe)=std::env::current_exe(){let _=Command::new(exe).args(["--watchdog",&std::process::id().to_string()]).creation_flags(0x08000000).spawn();}}s.hide=v;taskbar(v);reserve(&app,false);s.clone()}
#[tauri::command] fn quit(app:AppHandle){reserve(&app,false);taskbar(false);app.exit(0);}
fn main(){if std::env::args().nth(1).as_deref()==Some("--watchdog"){watchdog();return;}let mutex_name:Vec<u16>="Local\\NovaBarSingleInstance".encode_utf16().chain(Some(0)).collect();let single=unsafe{windows_sys::Win32::System::Threading::CreateMutexW(std::ptr::null(),0,mutex_name.as_ptr())};if single.is_null(){return;}if unsafe{windows_sys::Win32::Foundation::GetLastError()}==183{unsafe{windows_sys::Win32::Foundation::CloseHandle(single)};return;}tauri::Builder::default().setup(|app|{let mut s=std::fs::read_to_string(config(app.handle())).ok().and_then(|x|serde_json::from_str::<Settings>(&x).ok()).unwrap_or_else(defaults);s.hide=false;let initial=s.appearance.clone();app.manage(Data{settings:Mutex::new(s),system:Mutex::new(System::new()),last_foreground:Mutex::new(0),panel_open:Arc::new(AtomicBool::new(false))});let w=app.get_webview_window("main").unwrap();if let Some(m)=w.primary_monitor()?{let scale=m.scale_factor();let size=m.size();let pos=m.position();let width=(initial.width.max(1250) as f64*scale) as u32;let height=(108.0*scale) as u32;let width=width.min(size.width);w.set_size(PhysicalSize::new(width,height))?;w.set_position(PhysicalPosition::new(pos.x+(size.width-width) as i32/2,pos.y+(size.height-height) as i32))?;}start_edge_monitor(app.handle().clone());Ok(())}).invoke_handler(tauri::generate_handler![state,stats,launch,add,remove,hide,quit,media,media_action,network,icons,running,appearance,open_start,windows_list,activate_window,window_icons,installed_apps,launch_installed,system_panel,fullscreen,monitor_list,move_monitor,fullscreen_visibility,reorder,autostart,system_settings,volume_key,battery_details,dock_layout,remember_foreground,panel_state]).on_window_event(|w,e|{if matches!(e,tauri::WindowEvent::CloseRequested{..}){reserve(w.app_handle(),false);taskbar(false);}}).build(tauri::generate_context!()).expect("NovaBar startup failed").run(|_,e|{if matches!(e,tauri::RunEvent::Exit){taskbar(false);}});}

#[tauri::command] async fn media()->Result<serde_json::Value,String>{
tauri::async_runtime::spawn_blocking(|| -> Result<serde_json::Value,String>{
use windows::Media::Control::{GlobalSystemMediaTransportControlsSessionManager,GlobalSystemMediaTransportControlsSessionPlaybackStatus};
let manager=GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|x|x.get()).map_err(|e|e.to_string())?;
let session=match manager.GetCurrentSession(){Ok(s)=>s,Err(_)=>return Ok(serde_json::json!(null))};
let props=session.TryGetMediaPropertiesAsync().and_then(|x|x.get()).map_err(|e|e.to_string())?;
let artwork=(|| -> Option<String>{use windows::Storage::Streams::DataReader;use base64::Engine;let stream=props.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;let size=stream.Size().ok()?.min(4_000_000) as u32;if size==0{return None}let input=stream.GetInputStreamAt(0).ok()?;let reader=DataReader::CreateDataReader(&input).ok()?;let count=reader.LoadAsync(size).ok()?.get().ok()?;let mut bytes=vec![0u8;count as usize];reader.ReadBytes(&mut bytes).ok()?;let mime=if bytes.starts_with(&[137,80,78,71]){"image/png"}else if bytes.starts_with(&[255,216]){"image/jpeg"}else if bytes.starts_with(b"GIF"){"image/gif"}else{return None};Some(format!("data:{};base64,{}",mime,base64::engine::general_purpose::STANDARD.encode(bytes)))})();let playing=session.GetPlaybackInfo().and_then(|x|x.PlaybackStatus()).map(|x|x==GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing).unwrap_or(false);
Ok(serde_json::json!({"title":props.Title().map(|x|x.to_string_lossy()).unwrap_or_default(),"artist":props.Artist().map(|x|x.to_string_lossy()).unwrap_or_default(),"playing":playing,"artwork":artwork}))
}).await.map_err(|e|e.to_string())?
}
#[tauri::command] async fn media_action(action:String)->Result<(),String>{
tauri::async_runtime::spawn_blocking(move || -> Result<(),String>{
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
let m=GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|x|x.get()).map_err(|e|e.to_string())?;
let s=m.GetCurrentSession().map_err(|e|e.to_string())?;
let op=match action.as_str(){"previous"=>s.TrySkipPreviousAsync(),"next"=>s.TrySkipNextAsync(),"toggle"=>s.TryTogglePlayPauseAsync(),_=>return Err("Invalid media action".into())};
op.and_then(|x|x.get()).map_err(|e|e.to_string())?;Ok(())
}).await.map_err(|e|e.to_string())?
}
#[tauri::command] fn network()->serde_json::Value{
use windows::Networking::Connectivity::NetworkInformation;
if let Ok(p)=NetworkInformation::GetInternetConnectionProfile(){let wifi=p.IsWlanConnectionProfile().unwrap_or(false);let bars=p.GetSignalBars().and_then(|x|x.Value()).ok();let name=p.ProfileName().map(|x|x.to_string_lossy()).unwrap_or_default();return serde_json::json!({"connected":true,"wifi":wifi,"bars":bars,"name":name});}serde_json::json!({"connected":false})
}
#[tauri::command] async fn icons(data:State<'_,Data>)->Result<Vec<Option<String>>,String>{
let targets:Vec<String>=data.settings.lock().unwrap().shortcuts.iter().map(|x|x.target.clone()).collect();
tauri::async_runtime::spawn_blocking(move || targets.into_iter().map(|target|{
let path=if target=="explorer.exe"{std::env::var("WINDIR").ok().map(|x|format!("{}\\explorer.exe",x))?}else if target.starts_with("https:")||target.starts_with("ms-settings:"){return None}else{target};
let script=r#"Add-Type -AssemblyName System.Drawing; $p=$env:NOVABAR_ICON_PATH; if($p.EndsWith('.lnk')){$p=(New-Object -ComObject WScript.Shell).CreateShortcut($p).TargetPath}; $i=[System.Drawing.Icon]::ExtractAssociatedIcon($p); if($i){$b=$i.ToBitmap();$m=New-Object IO.MemoryStream;$b.Save($m,[System.Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($m.ToArray());$m.Dispose();$b.Dispose();$i.Dispose()}"#;
let o=Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",script]).env("NOVABAR_ICON_PATH",path).creation_flags(0x08000000).output().ok()?;if !o.status.success(){return None}let b=String::from_utf8(o.stdout).ok()?.trim().to_string();if b.is_empty(){None}else{Some(format!("data:image/png;base64,{}",b))}
}).collect()).await.map_err(|e|e.to_string())
}
#[tauri::command] fn running(data:State<Data>)->Vec<bool>{
let wins=windows_list();let s=data.settings.lock().unwrap();s.shortcuts.iter().map(|a|wins.iter().any(|w|matches_target(&a.target,&w.path))).collect()
}
#[tauri::command] fn appearance(app:AppHandle,data:State<Data>,value:Appearance)->Result<Settings,String>{
let mut s=data.settings.lock().unwrap();s.appearance=Appearance{width:value.width.clamp(650,1500),opacity:value.opacity.clamp(25,100),accent:value.accent,blur:value.blur};save(&app,&s);
let w=app.get_webview_window("main").ok_or("Missing window")?;
if let Some(m)=w.current_monitor().map_err(|e|e.to_string())?{let scale=m.scale_factor();let size=m.size();let pos=m.position();let width=((s.appearance.width.max(1250) as f64*scale) as u32).min(size.width);let height=(108.0*scale) as u32;w.set_size(PhysicalSize::new(width,height)).map_err(|e|e.to_string())?;w.set_position(PhysicalPosition::new(pos.x+(size.width-width) as i32/2,pos.y+(size.height-height) as i32)).map_err(|e|e.to_string())?;}if s.hide{reserve(&app,false);}Ok(s.clone())
}
#[tauri::command] fn open_start()->Result<(),String>{Command::new("explorer.exe").arg("shell:AppsFolder").spawn().map_err(|e|e.to_string())?;Ok(())}

#[derive(Serialize)] struct WindowInfo {id:String,title:String,path:String}
fn matches_target(target:&str,path:&str)->bool{if target.ends_with(".lnk")||target.contains(':')&& !target.contains('\\'){return false}let a=std::path::Path::new(target).file_name().unwrap_or_default().to_string_lossy();let b=std::path::Path::new(path).file_name().unwrap_or_default().to_string_lossy();!a.is_empty()&&a.eq_ignore_ascii_case(&b)}
#[tauri::command] fn windows_list()->Vec<WindowInfo>{
use windows_sys::Win32::{Foundation::{HWND,LPARAM,BOOL,CloseHandle},UI::WindowsAndMessaging::{EnumWindows,IsWindowVisible,GetWindowTextLengthW,GetWindowTextW,GetWindowThreadProcessId,GetWindow,GW_OWNER,GetWindowLongW,GWL_EXSTYLE,WS_EX_TOOLWINDOW},System::Threading::{OpenProcess,QueryFullProcessImageNameW,PROCESS_QUERY_LIMITED_INFORMATION}};
unsafe extern "system" fn collect(h:HWND,l:LPARAM)->BOOL{unsafe{
if IsWindowVisible(h)==0||!GetWindow(h,GW_OWNER).is_null()||(GetWindowLongW(h,GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW)!=0{return 1}
let n=GetWindowTextLengthW(h);if n<=0{return 1}let mut buf=vec![0u16;(n+1) as usize];let count=GetWindowTextW(h,buf.as_mut_ptr(),n+1);if count<=0{return 1}let title=String::from_utf16_lossy(&buf[..count as usize]);let mut pid=0;GetWindowThreadProcessId(h,&mut pid);if pid==std::process::id(){return 1}
let proc=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid);if proc.is_null(){return 1}let mut p=vec![0u16;32768];let mut len=p.len() as u32;let ok=QueryFullProcessImageNameW(proc,0,p.as_mut_ptr(),&mut len);CloseHandle(proc);if ok!=0{let path=String::from_utf16_lossy(&p[..len as usize]);let out=&mut *(l as *mut Vec<WindowInfo>);out.push(WindowInfo{id:(h as usize).to_string(),title,path});}1}}
let mut result=Vec::new();unsafe{EnumWindows(Some(collect),&mut result as *mut Vec<WindowInfo> as LPARAM);}result
}
#[tauri::command] fn activate_window(id:String,data:State<Data>)->Result<(),String>{let last=*data.last_foreground.lock().unwrap();activate_window_with_previous(id,last)}
fn activate_window_with_previous(id:String,last:usize)->Result<(),String>{
use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow,IsIconic,ShowWindow,SetForegroundWindow,GetForegroundWindow,SW_RESTORE,SW_MINIMIZE};let n=id.parse::<usize>().map_err(|e|e.to_string())?;if !windows_list().iter().any(|w|w.id==id){return Err("Window no longer available".into())}let h=n as windows_sys::Win32::Foundation::HWND;unsafe{if IsWindow(h)==0{return Err("Window closed".into())}if GetForegroundWindow()==h || (last!=0 && last==n){ShowWindow(h,SW_MINIMIZE);}else{if IsIconic(h)!=0{ShowWindow(h,SW_RESTORE);}SetForegroundWindow(h);}}Ok(())
}
#[tauri::command] async fn window_icons(paths:Vec<String>)->Result<Vec<Option<String>>,String>{
tauri::async_runtime::spawn_blocking(move || paths.into_iter().take(80).map(|p|{
let script=r#"Add-Type -AssemblyName System.Drawing; $i=[System.Drawing.Icon]::ExtractAssociatedIcon($env:NOVABAR_ICON_PATH); if($i){$b=$i.ToBitmap();$m=New-Object IO.MemoryStream;$b.Save($m,[System.Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($m.ToArray());$m.Dispose();$b.Dispose();$i.Dispose()}"#;
let o=Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",script]).env("NOVABAR_ICON_PATH",p).creation_flags(0x08000000).output().ok()?;if !o.status.success(){return None}let b=String::from_utf8(o.stdout).ok()?.trim().to_string();if b.is_empty(){None}else{Some(format!("data:image/png;base64,{}",b))}
}).collect()).await.map_err(|e|e.to_string())
}

fn watchdog(){use windows_sys::Win32::{Foundation::CloseHandle,System::Threading::{OpenProcess,WaitForSingleObject}};if let Some(pid)=std::env::args().nth(2).and_then(|x|x.parse::<u32>().ok()){unsafe{let h=OpenProcess(0x00100000,0,pid);if !h.is_null(){WaitForSingleObject(h,0xffffffff);CloseHandle(h);}}taskbar(false);}}
#[derive(Serialize,Deserialize)] struct InstalledApp { #[serde(rename="Name")] name:String, #[serde(rename="AppID")] id:String }
#[tauri::command] async fn installed_apps()->Result<Vec<InstalledApp>,String>{tauri::async_runtime::spawn_blocking(||{let o=Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command","[Console]::OutputEncoding=[Text.Encoding]::UTF8; @(Get-StartApps | Sort-Object Name) | ConvertTo-Json -Compress"]).creation_flags(0x08000000).output().map_err(|e|e.to_string())?;if !o.status.success(){return Err("Unable to enumerate installed apps".into())}serde_json::from_slice::<Vec<InstalledApp>>(&o.stdout).or_else(|_|serde_json::from_slice::<InstalledApp>(&o.stdout).map(|a|vec![a])).map_err(|e|e.to_string())}).await.map_err(|e|e.to_string())?}
#[tauri::command] fn launch_installed(id:String)->Result<(),String>{if id.is_empty()||id.contains('\n')||id.contains('\r'){return Err("Invalid application".into())}Command::new("explorer.exe").arg(format!("shell:AppsFolder\\{}",id)).spawn().map_err(|e|e.to_string())?;Ok(())}
#[tauri::command] fn system_panel(panel:String)->Result<(),String>{use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event,KEYEVENTF_KEYUP};let key=match panel.as_str(){"notifications"=>b'N',"quick"=>b'A',"start"=>0,"desktops"=>0x09,_=>return Err("Unknown panel".into())};unsafe{keybd_event(0x5b,0,0,0);if key!=0{keybd_event(key,0,0,0);keybd_event(key,0,KEYEVENTF_KEYUP,0);}keybd_event(0x5b,0,KEYEVENTF_KEYUP,0);}Ok(())}
#[tauri::command] fn fullscreen()->bool{use windows_sys::Win32::{Foundation::RECT,UI::WindowsAndMessaging::{GetForegroundWindow,GetWindowRect,GetWindowThreadProcessId},Graphics::Gdi::{MonitorFromWindow,GetMonitorInfoW,MONITORINFO,MONITOR_DEFAULTTONEAREST}};unsafe{let h=GetForegroundWindow();if h.is_null(){return false}let mut pid=0;GetWindowThreadProcessId(h,&mut pid);if pid==std::process::id(){return false}let mut r:RECT=std::mem::zeroed();if GetWindowRect(h,&mut r)==0{return false}let m=MonitorFromWindow(h,MONITOR_DEFAULTTONEAREST);let mut info:MONITORINFO=std::mem::zeroed();info.cbSize=std::mem::size_of::<MONITORINFO>() as u32;if GetMonitorInfoW(m,&mut info)==0{return false}let s=info.rcMonitor;r.left<=s.left&&r.top<=s.top&&r.right>=s.right&&r.bottom>=s.bottom}}
#[tauri::command] fn monitor_list(app:AppHandle)->Result<Vec<String>,String>{let w=app.get_webview_window("main").ok_or("Missing window")?;Ok(w.available_monitors().map_err(|e|e.to_string())?.iter().enumerate().map(|(i,m)|m.name().cloned().unwrap_or_else(||format!("Display {}",i+1))).collect())}
#[tauri::command] fn move_monitor(app:AppHandle,data:State<Data>,index:usize)->Result<(),String>{let w=app.get_webview_window("main").ok_or("Missing window")?;let monitors=w.available_monitors().map_err(|e|e.to_string())?;let m=monitors.get(index).ok_or("Display unavailable")?;let scale=m.scale_factor();let size=m.size();let pos=m.position();let width=((data.settings.lock().unwrap().appearance.width as f64*scale) as u32).min(size.width);let height=(108.0*scale) as u32;w.set_size(PhysicalSize::new(width,height)).map_err(|e|e.to_string())?;w.set_position(PhysicalPosition::new(pos.x+(size.width-width) as i32/2,pos.y+(size.height-height) as i32-(8.0*scale) as i32)).map_err(|e|e.to_string())?;if data.settings.lock().unwrap().hide{reserve(&app,false);reserve(&app,true);}Ok(())}

fn reserve(app:&AppHandle,enable:bool){use windows_sys::Win32::UI::Shell::{SHAppBarMessage,APPBARDATA,ABM_NEW,ABM_REMOVE,ABM_QUERYPOS,ABM_SETPOS,ABE_BOTTOM};if let Some(w)=app.get_webview_window("main"){if let Ok(hwnd)=w.hwnd(){unsafe{let mut d:APPBARDATA=std::mem::zeroed();d.cbSize=std::mem::size_of::<APPBARDATA>() as u32;d.hWnd=hwnd.0 as _;if !enable{SHAppBarMessage(ABM_REMOVE,&mut d);return;}d.uCallbackMessage=0x8001;SHAppBarMessage(ABM_NEW,&mut d);if let Ok(Some(m))=w.current_monitor(){let p=m.position();let s=m.size();let h=(94.0*m.scale_factor()) as i32;d.uEdge=ABE_BOTTOM;d.rc.left=p.x;d.rc.right=p.x+s.width as i32;d.rc.bottom=p.y+s.height as i32;d.rc.top=d.rc.bottom-h;SHAppBarMessage(ABM_QUERYPOS,&mut d);d.rc.top=d.rc.bottom-h;SHAppBarMessage(ABM_SETPOS,&mut d);}}}}}
#[tauri::command] fn fullscreen_visibility(app:AppHandle,hidden:bool)->Result<(),String>{let w=app.get_webview_window("main").ok_or("Missing window")?;w.set_ignore_cursor_events(hidden).map_err(|e|e.to_string())?;w.set_always_on_top(!hidden).map_err(|e|e.to_string())?;Ok(())}

#[tauri::command] fn reorder(app:AppHandle,data:State<Data>,from:usize,to:usize)->Settings{let mut s=data.settings.lock().unwrap();if from<s.shortcuts.len()&&to<s.shortcuts.len(){let item=s.shortcuts.remove(from);s.shortcuts.insert(to,item);save(&app,&s);}s.clone()}
#[tauri::command] fn autostart(enable:bool)->Result<(),String>{let folder=std::env::var("APPDATA").map_err(|e|e.to_string())?;let p=PathBuf::from(folder).join("Microsoft/Windows/Start Menu/Programs/Startup/NovaBar.lnk");if !enable{if p.exists(){std::fs::remove_file(p).map_err(|e|e.to_string())?;}return Ok(())}let exe=std::env::current_exe().map_err(|e|e.to_string())?;let script=r#"$s=(New-Object -ComObject WScript.Shell).CreateShortcut($env:NOVABAR_LINK);$s.TargetPath=$env:NOVABAR_EXE;$s.Save()"#;let o=Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",script]).env("NOVABAR_LINK",p).env("NOVABAR_EXE",exe).creation_flags(0x08000000).output().map_err(|e|e.to_string())?;if o.status.success(){Ok(())}else{Err(String::from_utf8_lossy(&o.stderr).into_owned())}}
#[tauri::command] fn system_settings(page:String)->Result<(),String>{let uri=match page.as_str(){"wifi"=>"ms-settings:network-wifi","sound"=>"ms-settings:sound","display"=>"ms-settings:display","battery"=>"ms-settings:batterysaver","notifications"=>"ms-settings:notifications","updates"=>"ms-settings:windowsupdate",_=>return Err("Unknown settings page".into())};Command::new("explorer.exe").arg(uri).spawn().map_err(|e|e.to_string())?;Ok(())}
#[tauri::command] fn volume_key(action:String)->Result<(),String>{use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event,KEYEVENTF_KEYUP};let k=match action.as_str(){"up"=>0xaf,"down"=>0xae,"mute"=>0xad,_=>return Err("Unknown volume action".into())};unsafe{keybd_event(k,0,0,0);keybd_event(k,0,KEYEVENTF_KEYUP,0);}Ok(())}
#[tauri::command] fn battery_details()->serde_json::Value{let mut p:windows_sys::Win32::System::Power::SYSTEM_POWER_STATUS=unsafe{std::mem::zeroed()};let ok=unsafe{windows_sys::Win32::System::Power::GetSystemPowerStatus(&mut p)};if ok==0{return serde_json::Value::Null}serde_json::json!({"charging":p.BatteryFlag&8!=0,"ac":p.ACLineStatus==1,"seconds":if p.BatteryLifeTime==u32::MAX{None}else{Some(p.BatteryLifeTime)},"percent":if p.BatteryLifePercent<=100{Some(p.BatteryLifePercent)}else{None}})}

#[tauri::command] fn dock_layout(app:AppHandle,position:String,expanded:bool)->Result<(),String>{
let w=app.get_webview_window("main").ok_or("Missing window")?;
let m=w.current_monitor().map_err(|e|e.to_string())?.or(w.primary_monitor().map_err(|e|e.to_string())?).ok_or("No monitor")?;
let scale=m.scale_factor();let area=m.size();let origin=m.position();
let s=app.state::<Data>();let configured=s.settings.lock().unwrap().appearance.width;
let width=((configured.max(1250) as f64*scale) as u32).min(area.width.saturating_sub((24.0*scale) as u32)).max(1);
let height=(((if expanded{620.0}else{108.0})*scale) as u32).min(area.height);
let x=match position.as_str(){"left"=>origin.x+(12.0*scale) as i32,"right"=>origin.x+(area.width-width) as i32-(12.0*scale) as i32,_=>origin.x+(area.width-width) as i32/2};
let y=origin.y+(area.height-height) as i32;
w.set_size(PhysicalSize::new(width,height)).map_err(|e|e.to_string())?;
w.set_position(PhysicalPosition::new(x,y)).map_err(|e|e.to_string())?;
Ok(())
}

#[tauri::command] fn remember_foreground(data:State<Data>){use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;let h=unsafe{GetForegroundWindow()};if h.is_null(){return;}let id=h as usize;if windows_list().iter().any(|w|w.id.parse::<usize>().ok()==Some(id)){*data.last_foreground.lock().unwrap()=id;}}

#[tauri::command] fn panel_state(data:State<Data>,open:bool){data.panel_open.store(open,Ordering::Relaxed);}
fn start_edge_monitor(app:AppHandle){
let flag=app.state::<Data>().panel_open.clone();
std::thread::spawn(move||{
use windows_sys::Win32::{Foundation::POINT,UI::WindowsAndMessaging::GetCursorPos};
let mut shown=false;let mut last_near=std::time::Instant::now();
loop{
std::thread::sleep(std::time::Duration::from_millis(65));
let Some(w)=app.get_webview_window("main") else {break};
let Ok(Some(m))=w.current_monitor() else {continue};
let p=m.position();let s=m.size();
let mut cursor:POINT=unsafe{std::mem::zeroed()};
if unsafe{GetCursorPos(&mut cursor)}==0{continue}
let at_edge=cursor.x>=p.x && cursor.x<p.x+s.width as i32 && cursor.y>=p.y+s.height as i32-3 && cursor.y<p.y+s.height as i32;
let inside=w.outer_position().ok().zip(w.outer_size().ok()).map(|(pos,size)|cursor.x>=pos.x && cursor.x<pos.x+size.width as i32 && cursor.y>=pos.y && cursor.y<pos.y+size.height as i32).unwrap_or(false);
let open=flag.load(Ordering::Relaxed);
if at_edge||inside||open{last_near=std::time::Instant::now();}
let should_show=(at_edge||open||shown && last_near.elapsed()<std::time::Duration::from_millis(850));
if should_show!=shown{
shown=should_show;
if shown{let _=w.show();}else{let _=w.hide();}
}
}
});
}
