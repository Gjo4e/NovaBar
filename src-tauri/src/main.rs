#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{sync::Mutex, path::PathBuf, process::Command, os::windows::process::CommandExt};
use serde::{Serialize,Deserialize};
use tauri::{Manager,State,AppHandle,PhysicalPosition,PhysicalSize};
use sysinfo::System;
#[derive(Clone,Serialize,Deserialize)]
struct Shortcut { name:String,target:String }
#[derive(Clone,Serialize,Deserialize)]
struct Settings { shortcuts:Vec<Shortcut>,hide:bool, #[serde(default)] appearance:Appearance }
#[derive(Clone,Serialize,Deserialize)] struct Appearance { width:u32, opacity:u8, accent:String, blur:bool }
impl Default for Appearance {fn default()->Self{Self{width:1100,opacity:72,accent:"#9c9cff".into(),blur:true}}}
struct Data { settings:Mutex<Settings>,system:Mutex<System> }
fn defaults()->Settings { Settings{hide:false,appearance:Appearance::default(),shortcuts:vec![Shortcut{name:"Explorer".into(),target:"explorer.exe".into()},Shortcut{name:"Browser".into(),target:"https://www.google.com".into()},Shortcut{name:"Settings".into(),target:"ms-settings:".into()}]} }
fn config(app:&AppHandle)->PathBuf { app.path().app_config_dir().unwrap().join("settings.json") }
fn save(app:&AppHandle,s:&Settings){let p=config(app);if let Some(d)=p.parent(){let _=std::fs::create_dir_all(d);}let _=std::fs::write(p,serde_json::to_string(s).unwrap());}
fn taskbar(hide:bool){unsafe{use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW,ShowWindow,SW_HIDE,SW_SHOW};let name:Vec<u16>="Shell_TrayWnd".encode_utf16().chain(Some(0)).collect();let h=FindWindowW(name.as_ptr(),std::ptr::null());if !h.is_null(){ShowWindow(h,if hide{SW_HIDE}else{SW_SHOW});}}}
#[tauri::command] fn state(data:State<Data>)->Settings{data.settings.lock().unwrap().clone()}
#[tauri::command] fn stats(data:State<Data>)->serde_json::Value{let mut s=data.system.lock().unwrap();s.refresh_cpu_usage();s.refresh_memory();let ram=100*s.used_memory()/s.total_memory().max(1);let mut power:windows_sys::Win32::System::Power::SYSTEM_POWER_STATUS=unsafe{std::mem::zeroed()};let ok=unsafe{windows_sys::Win32::System::Power::GetSystemPowerStatus(&mut power)};serde_json::json!({"cpu":s.global_cpu_usage().round() as u32,"ram":ram,"battery":if ok!=0 && power.BatteryLifePercent<=100 {Some(power.BatteryLifePercent)}else{None},"ac":ok!=0 && power.ACLineStatus==1})}
#[tauri::command] fn launch(i:usize,data:State<Data>)->Result<(),String>{let s=data.settings.lock().unwrap();let a=s.shortcuts.get(i).ok_or("Unknown shortcut")?;Command::new("explorer.exe").arg(&a.target).spawn().map_err(|e|e.to_string())?;Ok(())}
#[tauri::command] async fn add(app:AppHandle,data:State<'_,Data>)->Result<Settings,String>{let selected=tauri::async_runtime::spawn_blocking(||rfd::FileDialog::new().add_filter("Applications",&["exe","lnk"]).pick_file()).await.map_err(|e|e.to_string())?;let mut s=data.settings.lock().unwrap();if let Some(p)=selected{let name=p.file_stem().unwrap_or_default().to_string_lossy().into_owned();s.shortcuts.push(Shortcut{name,target:p.to_string_lossy().into_owned()});save(&app,&s);}Ok(s.clone())}
#[tauri::command] fn remove(i:usize,app:AppHandle,data:State<Data>)->Settings{let mut s=data.settings.lock().unwrap();if i<s.shortcuts.len(){s.shortcuts.remove(i);save(&app,&s);}s.clone()}
#[tauri::command] fn hide(v:bool,data:State<Data>)->Settings{let mut s=data.settings.lock().unwrap();s.hide=v;taskbar(v);s.clone()}
#[tauri::command] fn quit(app:AppHandle){taskbar(false);app.exit(0);}
fn main(){tauri::Builder::default().setup(|app|{let mut s=std::fs::read_to_string(config(app.handle())).ok().and_then(|x|serde_json::from_str::<Settings>(&x).ok()).unwrap_or_else(defaults);s.hide=false;app.manage(Data{settings:Mutex::new(s),system:Mutex::new(System::new())});let w=app.get_webview_window("main").unwrap();if let Some(m)=w.primary_monitor()?{let scale=m.scale_factor();let size=m.size();let pos=m.position();let width=(1100.0*scale) as u32;let height=(156.0*scale) as u32;let width=width.min(size.width);w.set_size(PhysicalSize::new(width,height))?;w.set_position(PhysicalPosition::new(pos.x+(size.width-width) as i32/2,pos.y+(size.height-height) as i32-(8.0*scale) as i32))?;}let _=window_vibrancy::apply_acrylic(&w,Some((20,22,34,100)));Ok(())}).invoke_handler(tauri::generate_handler![state,stats,launch,add,remove,hide,quit,media,media_action,network,icons,running,appearance,open_start]).on_window_event(|_,e|{if matches!(e,tauri::WindowEvent::CloseRequested{..}){taskbar(false);}}).build(tauri::generate_context!()).expect("NovaBar startup failed").run(|_,e|{if matches!(e,tauri::RunEvent::Exit){taskbar(false);}});}

#[tauri::command] async fn media()->Result<serde_json::Value,String>{
tauri::async_runtime::spawn_blocking(|| -> Result<serde_json::Value,String>{
use windows::Media::Control::{GlobalSystemMediaTransportControlsSessionManager,GlobalSystemMediaTransportControlsSessionPlaybackStatus};
let manager=GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|x|x.get()).map_err(|e|e.to_string())?;
let session=match manager.GetCurrentSession(){Ok(s)=>s,Err(_)=>return Ok(serde_json::json!(null))};
let props=session.TryGetMediaPropertiesAsync().and_then(|x|x.get()).map_err(|e|e.to_string())?;
let playing=session.GetPlaybackInfo().and_then(|x|x.PlaybackStatus()).map(|x|x==GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing).unwrap_or(false);
Ok(serde_json::json!({"title":props.Title().map(|x|x.to_string_lossy()).unwrap_or_default(),"artist":props.Artist().map(|x|x.to_string_lossy()).unwrap_or_default(),"playing":playing}))
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
let targets:Vec<String>=data.settings.lock().unwrap().shortcuts.iter().map(|x|x.target.clone()).collect();
let mut sys=data.system.lock().unwrap();sys.refresh_processes(sysinfo::ProcessesToUpdate::All,true);
targets.iter().map(|t|{let name=std::path::Path::new(t).file_name().unwrap_or_default().to_string_lossy();sys.processes().values().any(|p|p.name().to_string_lossy().eq_ignore_ascii_case(&name))}).collect()
}
#[tauri::command] fn appearance(app:AppHandle,data:State<Data>,value:Appearance)->Result<Settings,String>{
let mut s=data.settings.lock().unwrap();s.appearance=Appearance{width:value.width.clamp(650,1500),opacity:value.opacity.clamp(25,100),accent:value.accent,blur:value.blur};save(&app,&s);
let w=app.get_webview_window("main").ok_or("Missing window")?;if s.appearance.blur{let _=window_vibrancy::apply_acrylic(&w,Some((20,22,34,100)));}else{let _=window_vibrancy::clear_acrylic(&w);}
if let Some(m)=w.primary_monitor().map_err(|e|e.to_string())?{let scale=m.scale_factor();let size=m.size();let pos=m.position();let width=((s.appearance.width as f64*scale) as u32).min(size.width);let height=(156.0*scale) as u32;w.set_size(PhysicalSize::new(width,height)).map_err(|e|e.to_string())?;w.set_position(PhysicalPosition::new(pos.x+(size.width-width) as i32/2,pos.y+(size.height-height) as i32-(8.0*scale) as i32)).map_err(|e|e.to_string())?;}Ok(s.clone())
}
#[tauri::command] fn open_start()->Result<(),String>{Command::new("explorer.exe").arg("shell:AppsFolder").spawn().map_err(|e|e.to_string())?;Ok(())}
