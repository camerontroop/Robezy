#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{CustomMenuItem, SystemTray, SystemTrayMenu, SystemTrayMenuItem, SystemTrayEvent, Manager, Menu, Submenu};
use std::sync::{Arc, Mutex};

mod server_http;
mod server_ws;
mod fs_manager;
mod watcher;
mod plugin_manager;
mod robezy;

fn main() {
    let quit = CustomMenuItem::new("quit".to_string(), "Quit");
    let open = CustomMenuItem::new("open".to_string(), "Open Dashboard");
    let tray_menu = SystemTrayMenu::new()
        .add_item(open)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(quit);
    
    let tray = SystemTray::new().with_menu(tray_menu);

    // Application Menu
    let app_menu = Submenu::new("App", Menu::new()
        .add_native_item(tauri::MenuItem::Quit));
    
    let update_plugin = CustomMenuItem::new("update_plugin".to_string(), "Update Plugin");
    let file_menu = Submenu::new("File", Menu::new().add_item(update_plugin));
    
    let menu = Menu::new()
        .add_submenu(app_menu)
        .add_submenu(file_menu);

    tauri::Builder::default()
        .setup(|app| {
            // Create broadcast channel for logs
            use crate::server_ws::InternalBroadcast;
            use crate::server_ws::RobloxCommand;
            
            let (log_tx, _) = tokio::sync::broadcast::channel::<InternalBroadcast>(100);
            
            // Shared Command Queue
            let command_queue = Arc::new(Mutex::new(Vec::<RobloxCommand>::new()));
            
            tauri::async_runtime::spawn(server_http::start_server(log_tx.clone(), command_queue.clone()));

            // Check plugin on startup
            tauri::async_runtime::spawn(async {
                if let Err(e) = plugin_manager::ensure_installed().await {
                   eprintln!("Startup plugin check failed: {}", e);
                }
            });

            // --- Event Server (3031) ---
            tauri::async_runtime::spawn(server_ws::start_server(log_tx, command_queue));
            
            // --- RoBezy (Studio-First) Server (3032) ---
            use crate::robezy::session::SessionManager;
            let session_manager = Arc::new(Mutex::new(SessionManager::new()));
            tauri::async_runtime::spawn(robezy::server::start_robezy_server(session_manager, 3032));
            
            Ok(())
        })
        .menu(menu)
        .on_menu_event(|event| {
            match event.menu_item_id() {
                "update_plugin" => {
                    println!("Manual update triggered via menu");
                    let window = event.window().clone();
                    tauri::async_runtime::spawn(async move {
                        // RENAMED: install_plugins instead of install_rojo_plugin
                        match plugin_manager::install_plugins().await {
                            Ok(path) => {
                                println!("Plugin manually updated to: {}", path);
                                tauri::api::dialog::message(Some(&window), "Plugin Updated", format!("Successfully installed Rojo plugin to:\n{}", path));
                                
                                // Reveal in Finder using Tauri API
                                if let Some(parent) = std::path::Path::new(&path).parent() {
                                    let path_str = parent.to_string_lossy().to_string();
                                    if let Err(e) = tauri::api::shell::open(&window.shell_scope(), path_str, None) {
                                         eprintln!("Failed to open finder: {}", e);
                                         tauri::api::dialog::message(Some(&window), "Finder Error", format!("Could not open folder: {}", e));
                                    }
                                }
                            },
                            Err(e) => {
                                eprintln!("Failed to manually update plugin: {}", e);
                                tauri::api::dialog::message(Some(&window), "Update Failed", format!("Error installing plugin:\n{}", e));
                            },
                        }
                    });
                }
                _ => {}
            }
        })
        .system_tray(tray)
        .on_system_tray_event(|app, event| match event {
            SystemTrayEvent::LeftClick {
                position: _,
                size: _,
                ..
            } => {
                let window = app.get_window("main").unwrap();
                window.show().unwrap();
                window.set_focus().unwrap();
            }
            SystemTrayEvent::MenuItemClick { id, .. } => {
                match id.as_str() {
                    "quit" => {
                        std::process::exit(0);
                    }
                    "open" => {
                        let window = app.get_window("main").unwrap();
                        window.show().unwrap();
                        window.set_focus().unwrap();
                    }
                    _ => {}
                }
            }
            _ => {}
        })
        .on_window_event(|event| match event.event() {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                event.window().hide().unwrap();
                api.prevent_close();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
