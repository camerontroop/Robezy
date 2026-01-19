use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;
use futures_util::{StreamExt, SinkExt};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "sync:start")]
    SyncStart { projectId: String, projectName: String, files: Vec<FileEntry> },
    #[serde(rename = "sync:stop")]
    SyncStop,
    #[serde(rename = "file:update")]
    FileUpdate { path: String, content: String },
    #[serde(rename = "file:delete")]
    FileDelete { path: String },
    #[serde(rename = "query:instance")]
    QueryInstance { path: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileEntry {
    path: String,
    content: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum ServerMessage {
    #[serde(rename = "status")]
    Status { connected: bool },
    #[serde(rename = "file:changed")]
    FileChanged { path: String, content: String },
    #[serde(rename = "sync:ready")]
    SyncReady { projectPath: String },
    #[serde(rename = "plugin:log")]
    PluginLog { content: serde_json::Value },
    #[serde(rename = "workspace:map")]
    WorkspaceMap { services: serde_json::Value, hasTerrain: bool },
    #[serde(rename = "error")]
    Error { message: String },
    // NEW FOR DASHBOARD
    #[serde(rename = "project:sync")]
    ProjectSync { projectName: String, files: Vec<FileEntry> },
    #[serde(rename = "file:event")]
    FileEvent { path: String, content: Option<String>, kind: String },
    #[serde(rename = "project:stop")]
    ProjectStop,
    #[serde(rename = "query:result")]
    QueryResult { path: String, properties: serde_json::Value },
    #[serde(rename = "workspace:event")]
    WorkspaceEvent { content: serde_json::Value },
}

// Internal broadcast type
#[derive(Clone, Debug)]
pub enum InternalBroadcast {
    Log(serde_json::Value),
    Workspace(serde_json::Value, bool), // Legacy: tree, hasTerrain
    WorkspaceEvent(serde_json::Value), // New Two-Tier event
    ProjectSync { name: String, files: Vec<FileEntry> },
    FileEvent { path: String, content: Option<String>, kind: String, source_id: Option<u64> },
    ProjectStop { source_id: Option<u64> },
    QueryResult { path: String, properties: serde_json::Value },
}

// In a real app complexity, we'd inject this state or use a global.
// For simplicity here, we'll create a new RojoManager per connection or share one.
// Let's assume one active project at a time.
use std::sync::{Arc, Mutex};
use crate::fs_manager;
// use crate::rojo_manager::RojoManager; // DELETED
use crate::watcher::ProjectWatcher;
// Add a channel to send watcher events back to the main loop
use tokio::sync::mpsc;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH}; // Added imports
use std::thread;
// Add tokio broadcast
use tokio::sync::broadcast;

// Command Queue Types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RobloxCommand {
    pub id: String,
    pub command_type: String, // e.g. "query:instance"
    pub params: serde_json::Value,
}

pub type CommandQueue = Arc<Mutex<Vec<RobloxCommand>>>;

// Add SessionManager imports
use crate::robezy::session::{SessionManager, FileChange};

pub async fn start_server(log_rx: broadcast::Sender<InternalBroadcast>, command_queue: CommandQueue, session_manager: Arc<Mutex<SessionManager>>) {
    let port = 3031;
    println!("WebSocket server initializing on port {}", port);

    // Spawn a dedicated task to bridge FileEvents (Watcher) to SessionManager (Plugin Queue)
    let mut bridge_rx = log_rx.subscribe();
    let bridge_mgr = session_manager.clone();
    
    tauri::async_runtime::spawn(async move {
        while let Ok(msg) = bridge_rx.recv().await {
            if let InternalBroadcast::FileEvent { path, content, kind, source_id } = msg {
                // If source_id is None, it means the event came from the Disk Watcher (or System)
                // We must queue this for the Plugin to see.
                if source_id.is_none() && kind == "update" {
                    if let Some(mut mgr) = bridge_mgr.lock().ok() {
                         // We need to find which session owns this file
                         let sessions = mgr.get_all_sessions_meta(); // Get IDs first to avoid big iteration? 
                         // Check all sessions
                         // TODO: Optimization - Is there a better lookup? For now, linear scan is fine (few sessions).
                         
                         // We need to iterate mutable sessions to push to queue.
                         // SessionManager structure is: sessions: HashMap<String, Session>
                         // But 'mgr' is the MutexGuard. We can iterate directly.
                         
                         let mut target_session_id: Option<String> = None;
                         let mut relative_path = String::new();
                         let mut class_name = Some("ModuleScript".to_string()); // Default
                         
                         for (id, session) in &mgr.sessions {
                             if let Some(bound) = &session.bound_folder {
                                 // Check path containment
                                 if path.starts_with(bound) {
                                     // Found match!
                                     target_session_id = Some(id.clone());
                                     // Compute relative path
                                     // path: /Users/foo/Bar/Workspace/Part.server.lua
                                     // bound: /Users/foo/Bar
                                     // rel: Workspace/Part.server.lua
                                     if let Ok(rel) = std::path::Path::new(&path).strip_prefix(bound) {
                                         // Clean extension and infer class
                                         let filename = rel.file_name().unwrap_or_default().to_string_lossy().to_string();
                                         
                                         // Logic matches plugin_manager.rs expectations
                                         if filename.ends_with(".server.lua") {
                                             class_name = Some("Script".to_string());
                                         } else if filename.ends_with(".client.lua") {
                                             class_name = Some("LocalScript".to_string());
                                         } else if filename.ends_with(".lua") {
                                              class_name = Some("ModuleScript".to_string());
                                         }
                                         
                                         // Plugin expects a "Roblox Path" (e.g. Workspace.Part)?
                                         // NO. Plugin 'pollChanges' logic calls 'ensureInstance(change.path, ...)'
                                         // 'ensureInstance' splits by '/'.
                                         // So we should send "Workspace/Part.server.lua" (Relative FS Path).
                                         // The plugin function 'ensureInstance' cleans the extension itself.
                                         // So we just send the relative path as is.
                                         relative_path = rel.to_string_lossy().to_string();
                                     }
                                     break; // Only match one session
                                 }
                             }
                         }
                         
                         if let Some(sess_id) = target_session_id {
                             if let Some(session) = mgr.sessions.get_mut(&sess_id) {
                                 if let Ok(mut queue) = session.outbound_queue.lock() {
                                     queue.push(FileChange {
                                         path: relative_path,
                                         content: content.clone(),
                                         change_type: "write".to_string(),
                                         class_name: class_name,
                                         guid: None,
                                         is_script: true // Assume watcher only picks up scripts for now
                                     });
                                 }
                             }
                         }
                    }
                }
            }
        }
    });

    // IPv4 Listener
    let rx_v4_broadcast = log_rx.clone();
    let tx_broadcast = log_rx.clone(); // Need sender to broadcast sync events
    let queue_v4 = command_queue.clone();
    
    tauri::async_runtime::spawn(async move {
        let addr = format!("0.0.0.0:{}", port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                 println!("WS listening on IPv4: {}", addr);
                 while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(handle_connection(stream, rx_v4_broadcast.subscribe(), tx_broadcast.clone(), queue_v4.clone()));
                 }
            },
            Err(e) => eprintln!("Failed to bind WS IPv4: {}", e),
        }
    });
}

// Internal connection handler
async fn handle_connection(
    stream: TcpStream, 
    mut broadcast_rx: broadcast::Receiver<InternalBroadcast>,
    broadcast_tx: broadcast::Sender<InternalBroadcast>,
    command_queue: CommandQueue
) {
    let ws_stream = accept_async(stream).await.expect("Error during handshake");
    println!("New WebSocket connection");
    
    // Generate a simple Connection ID based on time
    let connection_id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;

    let (mut write, mut read) = ws_stream.split();
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(100);
    let watcher_handle: Arc<Mutex<Option<ProjectWatcher>>> = Arc::new(Mutex::new(None));

    // Send Initial Status
    let msg = ServerMessage::Status {
        connected: true,
    };
    let _ = tx.send(msg).await;
    let mut local_active_project: Option<String> = None; // Reset active project tracking since we aren't fetching it

    // TODO: If there is an active project, we might want to send its state to the dashboard immediately?
    // But we don't have the file cache here. The Dashboard will only catch NEW syncs for now unless we store state.
    // For now, "Live View" means watching events as they happen.

    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
    // let mut local_active_project = active_name;

    loop {
        tokio::select! {
             _ = interval.tick() => {
                 // LEGACY STATUS BROADCAST (Disabled for V3 UI Noise Reduction)
                 // let (connected, rojo_running, rojo_port) = {
                 //     let mut manager = rojo_manager.lock().unwrap();
                 //     (true, manager.is_running(), 34872)
                 // };
                 // let msg = ServerMessage::Status { connected, rojoRunning: rojo_running, rojoPort: rojo_port };
                 // let _ = write.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&msg).unwrap())).await;
             },
             Ok(msg) = broadcast_rx.recv() => {
                 let server_msg = match msg {
                     InternalBroadcast::Log(val) => Some(ServerMessage::PluginLog { content: val }),
                     InternalBroadcast::Workspace(val, has_terrain) => Some(ServerMessage::WorkspaceMap { services: val, hasTerrain: has_terrain }),
                     InternalBroadcast::ProjectSync { name, files } => Some(ServerMessage::ProjectSync { projectName: name, files }),
                     InternalBroadcast::FileEvent { path, content, kind, source_id } => {
                         if Some(connection_id) == source_id {
                             None
                         } else {
                             Some(ServerMessage::FileEvent { path, content, kind })
                         }
                     },
                     InternalBroadcast::ProjectStop { source_id } => {
                        if Some(connection_id) == source_id {
                            None
                        } else {
                            Some(ServerMessage::ProjectStop)
                        }
                     },
                     InternalBroadcast::QueryResult { path, properties } => {
                         Some(ServerMessage::QueryResult { path, properties })
                     },
                     InternalBroadcast::WorkspaceEvent(content) => {
                         println!("WS: Received WorkspaceEvent broadcast"); // TRACE
                         Some(ServerMessage::WorkspaceEvent { content })
                     }
                 };
                 if let Some(s_msg) = server_msg {
                     match serde_json::to_string(&s_msg) {
                         Ok(text) => {
                             println!("WS: Sending message size: {} bytes", text.len());
                             if let Err(e) = write.send(tokio_tungstenite::tungstenite::Message::Text(text)).await {
                                 println!("WS: Failed to write to socket: {}", e);
                             }
                         },
                         Err(e) => println!("WS: Failed to serialize message: {}", e),
                     }
                 }
             },
             Some(msg) = read.next() => {
                 match msg {
                    Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                        if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                            match client_msg {
                                ClientMessage::SyncStart { projectId, projectName, files } => {
                                    let unique_name = format!("{}_{}", projectName, projectId);
                                    println!("Starting sync for {} (Dir: {}) with {} files", projectName, unique_name, files.len());
                                    local_active_project = Some(unique_name.clone());
                                    
                                    // 1. Broadcast Sync Event (so Dashboard sees it)
                                    // Note: files is consumed here. We need to clone specific data for broadcast if we use files later.
                                    // Actually we write files first, then broadcast? Or broadcast first?
                                    // Broadcast first so Dashboard can prepare UI.
                                    // We need to clone `files` to broadcast it AND use it.
                                    // `FileEntry` needs Clone derive. But it is inside this file. I'll add Clone above.
                                    
                                    // HACK: Re-construct files for broadcast to avoid adding Clone to FileEntry if strict.
                                    // But adding `#[derive(Clone)]` to FileEntry struct is easier. I will assume I can do that.
                                    
                                    // ... Logic to create project ...
                                    if let Ok(path) = fs_manager::create_project(&unique_name) {
                                        for file in &files {
                                            let _ = fs_manager::write_file(&unique_name, &file.path, &file.content);
                                        }
                                        
                                        // BROADCAST
                                        let _ = broadcast_tx.send(InternalBroadcast::ProjectSync { 
                                            name: unique_name.clone(), 
                                            files: files.iter().map(|f| FileEntry { path: f.path.clone(), content: f.content.clone() }).collect() 
                                        });

                                        // ... Watcher and Rojo startup logic ...
                                        // (Existing logic preserved, check replaced block)
                                        // ... Watcher and Rojo startup logic ...
                                        let broadcast_tx_clone = broadcast_tx.clone();
                                        let _ = ProjectWatcher::new(unique_name.clone(), move |path, content| {
                                             // Broadcast to ALL clients (Dashboard included)
                                             // source_id = None means "System/Watcher Event"
                                             let _ = broadcast_tx_clone.send(InternalBroadcast::FileEvent { 
                                                 path, 
                                                 content: Some(content), 
                                                 kind: "update".to_string(),
                                                 source_id: None 
                                             });
                                        });
                                        // ...
                                        
                                        // Generate default.project.json
                                        let project_json = format!(r#"{{
                                          "name": "{}",
                                          "tree": {{
                                            "$className": "DataModel",
                                            "ServerScriptService": {{
                                              "$className": "ServerScriptService",
                                              "$path": "src/ServerScriptService"
                                            }},
                                            "StarterPlayer": {{
                                              "$className": "StarterPlayer",
                                              "StarterPlayerScripts": {{
                                                "$className": "StarterPlayerScripts",
                                                "$path": "src/StarterPlayerScripts"
                                              }}
                                            }},
                                            "ReplicatedStorage": {{
                                              "$className": "ReplicatedStorage",
                                              "$path": "src/ReplicatedStorage",
                                              "$ignoreUnknownInstances": true
                                            }}
                                          }}
                                        }}"#, projectName);

                                        // WRITE TO DISK so watcher sees it
                                        // Use write_file from fs_manager
                                        let _ = fs_manager::write_file(&unique_name, "default.project.json", &project_json);

                                        // BROADCAST THE CREATION EXPLICITLY TO BE SAFE
                                        // This ensures the dashboard gets the updated file immediately
                                        let _ = broadcast_tx.send(InternalBroadcast::FileEvent {
                                            path: "default.project.json".to_string(),
                                            content: Some(project_json.clone()),
                                            kind: "update".to_string(),
                                            source_id: None // System event, send to everyone
                                        });
                                        
                                        // ... Start Rojo ...
                                        // Rojo Start Removed
                                        let msg_to_send = ServerMessage::Status { connected: true };
                                        let _ = write.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&msg_to_send).unwrap())).await;
                                        
                                        let ready_msg = ServerMessage::SyncReady { projectPath: path.to_string_lossy().to_string() };
                                        let _ = write.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&ready_msg).unwrap())).await;

                                    } else {
                                         // Error
                                    }
                                },
                                ClientMessage::SyncStop => {
                                    // Rojo Stop Removed
                                    let msg_to_send = ServerMessage::Status { connected: true };
                                    local_active_project = None;
                                    
                                    // BROADCAST STOP
                                    let _ = broadcast_tx.send(InternalBroadcast::ProjectStop { source_id: Some(connection_id) });

                                    let _ = write.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&msg_to_send).unwrap())).await;
                                },
                                ClientMessage::FileUpdate { path, content } => {
                                     // BROADCAST
                                     let _ = broadcast_tx.send(InternalBroadcast::FileEvent { 
                                         path: path.clone(), 
                                         content: Some(content.clone()), 
                                         kind: "update".to_string(),
                                         source_id: Some(connection_id)
                                     });
                                     
                                     let current_project = local_active_project.clone();
                                     if let Some(name) = current_project {
                                        let _ = fs_manager::write_file(&name, &path, &content);
                                     }
                                },
                                ClientMessage::FileDelete { path } => {
                                     // BROADCAST
                                     let _ = broadcast_tx.send(InternalBroadcast::FileEvent { 
                                         path: path.clone(), 
                                         content: None, 
                                         kind: "delete".to_string(),
                                         source_id: Some(connection_id)
                                     });
                                     
                                     let current_project = local_active_project.clone();
                                     if let Some(name) = current_project {
                                        let _ = fs_manager::delete_file(&name, &path);
                                     }
                                },
                                ClientMessage::QueryInstance { path } => {
                                    // Add to command queue
                                    let id = uuid::Uuid::new_v4().to_string();
                                    let cmd = RobloxCommand {
                                        id,
                                        command_type: "query:instance".to_string(),
                                        params: serde_json::json!({ "path": path })
                                    };
                                    
                                    if let Ok(mut queue) = command_queue.lock() {
                                        queue.push(cmd);
                                    }
                                },
                            }
                        }
                    }
                    Ok(_) => {},
                    Err(_) => break,
                 }
             },
             Some(server_msg) = rx.recv() => {
                 let _ = write.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&server_msg).unwrap())).await;
             }
             else => break,
        }
    }
}
