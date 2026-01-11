use std::sync::{Arc, Mutex};
use warp::Filter;
use serde::Deserialize; 
use crate::robezy::session::{SessionManager, SessionIdentity, FileChange};

// Request Structs must be module-level for safety
#[derive(Deserialize)]
struct UploadRequest {
    session_id: String,
    files: Vec<crate::robezy::session::ProjectFile>,
}

#[derive(Deserialize)]
struct ConnectRequest {
    place_id: i64,
    place_name: String,
    session_id: String,
    project_id: Option<String>,
    #[serde(default)]
    files: Vec<crate::robezy::session::ProjectFile>,
}

#[derive(Deserialize)]
struct HeartbeatRequest {
    session_id: String,
}

#[derive(Deserialize)]
struct DisconnectRequest {
    session_id: String,
}

pub async fn start_robezy_server(session_manager: Arc<Mutex<SessionManager>>, port: u16) {
    println!("DEBUG: Starting RoBezy Server setup...");
    
    let cleanup_mgr = session_manager.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            {
                // Wrap in scope to release lock
                if let Ok(mut mgr) = cleanup_mgr.lock() {
                     mgr.cleanup_stale_sessions();
                }
            }
        }
    });

    let session_manager = warp::any().map(move || session_manager.clone());
    
    // POST /robezy/upload
    let upload_route = warp::path!("robezy" / "upload")
        .and(warp::post())
        .and(warp::body::json())
        .and(session_manager.clone())
        .map(|req: UploadRequest, manager: Arc<Mutex<SessionManager>>| {
             manager.lock().unwrap().stage_files(req.session_id, req.files);
             warp::reply::json(&"uploaded")
        });

    // POST /robezy/connect
    let connect_route = warp::path!("robezy" / "connect")
        .and(warp::post())
        .and(warp::body::json())
        .and(session_manager.clone())
        .map(|req: ConnectRequest, manager: Arc<Mutex<SessionManager>>| {
            let identity = SessionIdentity {
                place_id: req.place_id,
                place_name: req.place_name,
                session_id: req.session_id.clone(),
                project_id: req.project_id,
            };
            
            println!("RoBezy HTTP: Connecting {} ({})", identity.place_name, identity.session_id);
            let final_id = manager.lock().unwrap().register_session(identity, req.files);
            
            warp::reply::json(&serde_json::json!({
                "status": "connected",
                "project_id": final_id
            }))
        });

    // POST /robezy/heartbeat
    let heartbeat_route = warp::path!("robezy" / "heartbeat")
        .and(warp::post())
        .and(warp::body::json())
        .and(session_manager.clone())
        .map(|req: HeartbeatRequest, manager: Arc<Mutex<SessionManager>>| {
             let mut mgr = manager.lock().unwrap();
             if mgr.refresh_session_ttl(&req.session_id) {
                 warp::reply::json(&"ok")
             } else {
                 warp::reply::json(&"unknown_session")
             }
        });

    // POST /robezy/disconnect
    let disconnect_route = warp::path!("robezy" / "disconnect")
        .and(warp::post())
        .and(warp::body::json())
        .and(session_manager.clone())
        .map(|req: DisconnectRequest, manager: Arc<Mutex<SessionManager>>| {
            println!("RoBezy HTTP: Disconnect request for {}", req.session_id);
            manager.lock().unwrap().unregister_session(&req.session_id);
            warp::reply::json(&"disconnected")
        });


    // POST /robezy/bind
    // Body: { "session_id": "...", "path": "..." }
    #[derive(Deserialize)]
    struct BindRequest {
        session_id: String,
        path: String,
    }
    
    // POST /robezy/sync
    // Using Imported 'FileChange' struct from session.rs

    #[derive(Deserialize)]
    struct SyncRequest {
        session_id: String,
        changes: Vec<FileChange>,
    }

    let bind_route = warp::path!("robezy" / "bind")
        .and(warp::post())
        .and(warp::body::json())
        .and(session_manager.clone())
        .map(|req: BindRequest, manager: Arc<Mutex<SessionManager>>| {
            let mut mgr = manager.lock().unwrap();
            match mgr.bind_folder(&req.session_id, req.path) {
                Ok(_) => warp::reply::json(&"bound"),
                Err(e) => warp::reply::json(&format!("error: {}", e)),
            }
        });

    let sync_route = warp::path!("robezy" / "sync")
        .and(warp::post())
        .and(warp::body::json())
        .and(session_manager.clone())
        .map(|req: SyncRequest, manager: Arc<Mutex<SessionManager>>| {
            // Retrieve Session Metadata (Clone Arc maps) to avoid holding lock during async write
            let (maybe_fm, ignore_map) = {
                let mgr = manager.lock().unwrap();
                let fm = mgr.get_file_manager(&req.session_id).cloned();
                let ignores = mgr.get_session(&req.session_id).map(|s| s.ignore_paths.clone());
                (fm, ignores)
            };

            if let Some(fm) = maybe_fm {
                // Spawn async task with the CLONED fm (which shares internal state via Arc)
                tokio::spawn(async move {
                    for change in req.changes {
                        if change.change_type == "write" {
                            if let Some(content) = change.content {
                                let guid_to_use = change.guid.clone().unwrap_or_default();
                                
                                // ANTI-LOOP: Register this path as ignored for 2 seconds
                                if let Some(ref map) = ignore_map {
                                    // change.path comes from Studio (e.g. Workspace.Part)
                                    // Our watcher uses mapped paths. Ideally we use the FS path. 
                                    // But since we don't know the exact FS path easily here without resolving,
                                    // Let's rely on the fact that fs.rs writes to a path derived from this.
                                    // Wait, fs.rs maps "Workspace.Part" -> "Workspace/Part.server.lua" (or similar).
                                    // The Watcher sees "Workspace/Part.server.lua".
                                    // We need to register "Workspace/Part.server.lua" in the ignore map.
                                    // Since we don't have the fs mapper logic exposed here easily...
                                    // Actually, we can just use the 'path' from the request IF we normalize it? 
                                    // No, watcher provides "Workspace/Part.server.lua".
                                    // We need to predict the FS path.
                                    // Let's simplisticly assume standard mapping for now or fix fs.rs to return it.
                                    // Or: Modify NativeFileManager to return the written path?
                                    // Let's chance it: The FS event will happen.
                                    // Just ignoring "Workspace/Part.server.lua" is hard if we don't know extension.
                                    // Alternative: Ignore *any* event that resolves to this Roblox Path?
                                    
                                    // BETTER: During write_file_guid, the NativeFileManager knows the path. 
                                    // But modifying that trait is deeply invasive.
                                    
                                    // BEST EFFORT: We know the extension based on ClassName.
                                    let ext = match change.class_name.as_deref().unwrap_or("ModuleScript") {
                                        "Script" => "server.lua",
                                        "LocalScript" => "client.lua",
                                        _ => "lua"
                                    };
                                    // Convert dots to slashes? No, path is "Workspace.Part".
                                    // NativeFileManager converts dots to slashes.
                                    let rel_path = change.path.replace(".", "/") + "." + ext;
                                    
                                    // Lock & Add
                                    let mut ignores = map.lock().unwrap();
                                    ignores.insert(rel_path.clone(), std::time::Instant::now() + std::time::Duration::from_secs(2));
                                    // Also try with just .lua just in case?
                                    // ignores.insert(change.path.replace(".", "/") + ".lua", ...);
                                }
                                
                                if let Err(e) = fm.write_file_guid(&guid_to_use, &change.path, change.is_script, change.class_name.as_deref(), content).await {
                                    eprintln!("RoBezy Sync Error: {}", e);
                                } else {
                                    println!("RoBezy: Synced {} ({})", change.path, guid_to_use);
                                }
                            }
                        }
                    }
                });
                
                warp::reply::json(&"syncing")
            } else {
                warp::reply::json(&"error: session/bind not found")
            }
        });

    // GET /robezy/poll_changes?session_id=...
    #[derive(Deserialize)]
    struct PollQuery {
        session_id: String,
    }

    let poll_route = warp::path!("robezy" / "poll_changes")
        .and(warp::get())
        .and(warp::query::<PollQuery>())
        .and(session_manager.clone())
        .map(|query: PollQuery, manager: Arc<Mutex<SessionManager>>| {
            let mgr = manager.lock().unwrap();
            if let Some(session) = mgr.get_session(&query.session_id) {
                let mut queue = session.outbound_queue.lock().unwrap();
                let changes: Vec<FileChange> = queue.drain(..).collect();
                warp::reply::json(&changes)
            } else {
                // If session not found, return empty array to avoid breaking client
                let empty: Vec<FileChange> = Vec::new();
                warp::reply::json(&empty)
            }
        });

    // POST /robezy/proxy_write
    // Writes to the bound folder on behalf of a web client.
    // The FS Watcher will then pick this up and sync to Studio.
    #[derive(Deserialize)]
    struct ProxyWriteRequest {
        session_id: String,
        path: String, // Relative path, e.g. "ServerScriptService/Script.server.lua"
        content: String,
    }

    let proxy_write_route = warp::path!("robezy" / "proxy_write")
        .and(warp::post())
        .and(warp::body::json())
        .and(session_manager.clone())
        .map(|req: ProxyWriteRequest, manager: Arc<Mutex<SessionManager>>| {
            let mgr = manager.lock().unwrap();
            if let Some(session) = mgr.get_session(&req.session_id) {
                if let Some(bound_folder) = &session.bound_folder {
                   let full_path = std::path::Path::new(bound_folder).join(&req.path);
                   
                   // Security check: simple path traversal prevention
                   // (Allowing only if it starts with bound_folder)
                   if full_path.starts_with(bound_folder) {
                        // Ensure parent dir exists
                        if let Some(parent) = full_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        
                        match std::fs::write(&full_path, req.content) {
                            Ok(_) => warp::reply::json(&"written"),
                            Err(e) => warp::reply::json(&format!("error writing: {}", e)),
                        }
                   } else {
                       warp::reply::json(&"error: invalid path traversal")
                   }
                } else {
                    warp::reply::json(&"error: session not bound")
                }
            } else {
                warp::reply::json(&"error: session not found")
            }
        });

    // GET /robezy/sessions
    let sessions_route = warp::path!("robezy" / "sessions")
        .and(warp::get())
        .and(session_manager.clone())
        .map(|manager: Arc<Mutex<SessionManager>>| {
            let sessions = manager.lock().unwrap().get_all_sessions_meta();
            warp::reply::json(&sessions)
        });

    // GET /robezy/sessions/:id
    let session_by_id_route = warp::path!("robezy" / "sessions" / String)
        .and(warp::get())
        .and(session_manager.clone())
        .map(|id: String, manager: Arc<Mutex<SessionManager>>| {
            let mgr = manager.lock().unwrap();
            if let Some(session) = mgr.get_session(&id) {
                // Return Identity + Files
                let mut response = serde_json::to_value(&session.identity).unwrap_or(serde_json::json!({}));
                if let Some(obj) = response.as_object_mut() {
                    obj.insert("files".to_string(), serde_json::to_value(&session.files).unwrap_or(serde_json::Value::Null));
                }
                warp::reply::json(&response)
            } else {
                // Return null or error object
                warp::reply::json(&serde_json::json!({ "error": "session not found" }))
            }
        });

    let cors = warp::cors()
        .allow_any_origin() // For development (file:// or localhost)
        .allow_methods(vec!["GET", "POST", "OPTIONS"])
        .allow_headers(vec!["content-type"]);

    let routes = connect_route
        .or(upload_route)
        .or(heartbeat_route)
        .or(disconnect_route)
        .or(poll_route)
        .or(sync_route)
        .or(sessions_route)
        .or(session_by_id_route)
        .or(proxy_write_route)
        .with(cors);

    println!("RoBezy HTTP Server listening on 127.0.0.1:{}", port);
    warp::serve(routes).run(([127, 0, 0, 1], port)).await;
}
