#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{sync::Mutex, path::PathBuf, process::Command};
use serde::{Serialize,Deserialize};
use tauri::{Manager,State,AppHandle,PhysicalPosition,PhysicalSize};
use sysinfo::System;
#[derive(Clone,Serialize,Deserialize)]
struct Shortcut { name:String,target:String }
#[derive(Clone,Serialize,Deserialize)]
struct Settings { shortcuts:Vec<Shortcut>,hide:bool }
struct Data { settings:Mutex<Settings>,system:Mutex<System> }
fn defaults()->Settings { Settings{hide:false,shortcuts:vec![Shortcut{name:"Explorer".into(),target:"explorer.exe".into()},Shortcut{name:"Browser".into(),target:"https://www.google.com".into()},Shortcut{name:"Settings".into(),target:"ms-settings:".into()}]} }
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
fn main(){tauri::Builder::default().setup(|app|{let mut s=std::fs::read_to_string(config(app.handle())).ok().and_then(|x|serde_json::from_str::<Settings>(&x).ok()).unwrap_or_else(defaults);s.hide=false;app.manage(Data{settings:Mutex::new(s),system:Mutex::new(System::new_all())});let w=app.get_webview_window("main").unwrap();if let Some(m)=w.primary_monitor()?{let scale=m.scale_factor();let size=m.size();let pos=m.position();let width=(900.0*scale) as u32;let height=(120.0*scale) as u32;let width=width.min(size.width);w.set_size(PhysicalSize::new(width,height))?;w.set_position(PhysicalPosition::new(pos.x+(size.width-width) as i32/2,pos.y+(size.height-height) as i32-(8.0*scale) as i32))?;}Ok(())}).invoke_handler(tauri::generate_handler![state,stats,launch,add,remove,hide,quit]).on_window_event(|_,e|{if matches!(e,tauri::WindowEvent::CloseRequested{..}){taskbar(false);}}).build(tauri::generate_context!()).expect("NovaBar startup failed").run(|_,e|{if matches!(e,tauri::RunEvent::Exit){taskbar(false);}});}
