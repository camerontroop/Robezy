use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::net::SocketAddr;
use serde::{Serialize, Deserialize};
use tokio::sync::mpsc;
use uuid::Uuid;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionIdentity {
    pub place_id: i64,
    pub place_name: String,
    pub session_id: String, // UUID from plugin
    pub project_id: Option<String>,
}

use crate::robezy::fs::NativeFileManager;

use notify::RecommendedWatcher;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub change_type: String, // "write", "delete"
    pub path: String,        // "Workspace.Folder.Script" or relative file path
    pub content: Option<String>,
    pub is_script: bool,
    pub guid: Option<String>, // Optional coming from FS (might not know GUID)
    pub class_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug)]
pub struct Session {
    pub identity: SessionIdentity,
    pub bound_folder: Option<String>,
    pub outbound_queue: Arc<Mutex<Vec<FileChange>>>, // Queue for Studio to poll
    // Map Path -> Expiration Time (Ignore writes from backend to avoid loop)
    pub ignore_paths: Arc<Mutex<HashMap<String, std::time::Instant>>>,
    pub watcher: Option<RecommendedWatcher>, // Keep watcher alive
    pub last_heartbeat: std::time::Instant,
    pub files: Vec<ProjectFile>, // Initial snapshot + updates? Actually just initial for now.
}

impl Session {
    pub fn new(identity: SessionIdentity, files: Vec<ProjectFile>) -> Self {
        Self {
            identity,
            bound_folder: None,
            outbound_queue: Arc::new(Mutex::new(Vec::new())),
            ignore_paths: Arc::new(Mutex::new(HashMap::new())),
            watcher: None,
            last_heartbeat: std::time::Instant::now(),
            files,
        }
    }
}

pub struct SessionManager {
    pub sessions: HashMap<String, Session>,
    file_managers: HashMap<String, NativeFileManager>,
    staging_files: HashMap<String, Vec<ProjectFile>>, // Temporary storage for chunked uploads
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            file_managers: HashMap::new(),
            staging_files: HashMap::new(),
        }
    }

    pub fn stage_files(&mut self, session_id: String, mut files: Vec<ProjectFile>) {
        let entry = self.staging_files.entry(session_id).or_insert_with(Vec::new);
        entry.append(&mut files);
        println!("RoBezy Staging: Accumulated {} files for session", entry.len());
    }

    pub fn register_session(&mut self, mut identity: SessionIdentity, mut files: Vec<ProjectFile>) -> String {
        // Merge with staged files
        if let Some(mut staged) = self.staging_files.remove(&identity.session_id) {
            println!("RoBezy: Merging {} staged files into connection", staged.len());
            staged.append(&mut files);
            files = staged;
        }
        // DEDUPLICATION: Remove any existing sessions for this Project Check
        let mut sessions_to_remove = Vec::new();
        // ... (Dedup logic removed for brevity in snippet, but we keep it logically if possible, or simpler: handle after ID resolution)
        // Wait, I need to resolve ID first to dedup correctly!
        
        let mut resolved_id = identity.project_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut final_folder_path: Option<std::path::PathBuf> = None;

        if let Some(mut docs) = dirs::document_dir() {
            docs.push("RobloxProjects");
            let safe_name = identity.place_name.chars()
                .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
                .collect::<String>();
            let safe_name = safe_name.trim().to_string();
            
            // Strategy:
            // 1. If explicit ID provided, bind to {Name}_{Prefix}.
            // 2. If NO ID provided, bind to {Name} (Stable Folder).
            //    - If {Name} exists & has ID -> Use it.
            //    - If {Name} doesn't exist used -> Create & Assign new ID.

            if let Some(provided_id) = &identity.project_id {
                // Legacy/Explicit Mode
                let unique_suffix = if provided_id.len() >= 8 { &provided_id[0..8] } else { provided_id };
                let folder_name = format!("{}_{}", safe_name, unique_suffix);
                final_folder_path = Some(docs.join(folder_name));
            } else {
                // Stable Mode: Smart Uniqueness Loop
                let mut found_path: Option<PathBuf> = None;
                let mut attempt = 0;
                let max_attempts = 100; // Prevent infinite loops

                while attempt < max_attempts {
                    let suffix = if attempt == 0 { "".to_string() } else { format!("_{}", attempt) };
                    let candidate_name = format!("{}{}", safe_name, suffix);
                    let candidate_path = docs.join(&candidate_name);
                    let id_file = candidate_path.join("robezy.id");
                    
                    if !candidate_path.exists() {
                        // Case A: New Folder -> CLAIM IT
                        let _ = std::fs::create_dir_all(&candidate_path);
                        let _ = std::fs::write(&id_file, &resolved_id);
                        found_path = Some(candidate_path);
                        break;
                    } else if !id_file.exists() {
                        // Case B: Folder exists but no Owner -> CLAIM IT
                        // (Assume it's an abandoned folder or a user manually created one)
                        let _ = std::fs::write(&id_file, &resolved_id);
                        found_path = Some(candidate_path);
                        break;
                    } else {
                        // Case C: Folder exists AND has Owner -> CHECK IT
                        if let Ok(content) = std::fs::read_to_string(&id_file) {
                             let stored_id = content.trim().to_string();
                             if stored_id == resolved_id {
                                 // It's OUR folder! -> BIND
                                 found_path = Some(candidate_path);
                                 // Ensure identity carries this persistent ID now
                                 identity.project_id = Some(resolved_id.clone());
                                 break;
                             } else {
                                 // It's THEIR folder! -> SKIP
                                 // Loop continues to next suffix
                             }
                        } else {
                            // Read error? Treat as claimed/locked -> SKIP
                        }
                    }
                    attempt += 1;
                }
                
                final_folder_path = found_path;
            }
        }

        // Now we have the Resolved ID. Run Dedup.
        for (id, s) in &self.sessions {
            if let Some(old_pid) = &s.identity.project_id {
                if old_pid == &resolved_id {
                    println!("RoBezy Dedup: Removing stale session {} (Match: {})", id, resolved_id);
                    sessions_to_remove.push(id.clone());
                }
            }
        }
        for id in sessions_to_remove {
            self.unregister_session(&id);
        }
        
        // Update Session Identity with Resolved ID
        identity.project_id = Some(resolved_id.clone());

        println!("RoBezy: Registering request for {} ({})", identity.place_name, identity.session_id);
        
        // Use the Updated Identity
        let mut session = Session::new(identity.clone(), files.clone());
        
        // BINDING
        if let Some(path) = final_folder_path {
            let path_str = path.to_string_lossy().to_string();
            println!("RoBezy: Auto-binding to {}", path_str);
            let _ = std::fs::create_dir_all(&path);
            
             // WRITE INITIAL FILES
            for file in &files {
                let full_path = path.join(&file.path);
                if let Some(parent) = full_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(&full_path, &file.content) {
                    eprintln!("RoBezy: Failed to write initial file {}: {}", file.path, e);
                }
            }
            
            session.bound_folder = Some(path_str.clone());
            let fm = NativeFileManager::new(path.clone());
            self.file_managers.insert(identity.session_id.clone(), fm);
             // START WATCHER
            session.watcher = setup_watcher(path_str, session.outbound_queue.clone(), session.ignore_paths.clone());
        }

        self.sessions.insert(identity.session_id, session);
        return resolved_id;
    }

    pub fn unregister_session(&mut self, session_id: &str) {
        if let Some(s) = self.sessions.remove(session_id) {
            self.file_managers.remove(session_id);
            println!("RoBezy: Unregistered session {} (Place {})", s.identity.session_id, s.identity.place_id);
        }
    }

    pub fn bind_folder(&mut self, session_id: &str, folder_path: String) -> Result<(), String> {
        if let Some(session) = self.sessions.get_mut(session_id) {
            println!("RoBezy: Binding session {} to folder {}", session_id, folder_path);
            session.bound_folder = Some(folder_path.clone());
            
            // Create and store manager
            let fm = NativeFileManager::new(folder_path.clone());
            self.file_managers.insert(session_id.to_string(), fm);
            
            // START WATCHER (Replace existing if any)
            session.watcher = setup_watcher(folder_path, session.outbound_queue.clone(), session.ignore_paths.clone());
            
            Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }
    
    pub fn get_file_manager(&self, session_id: &str) -> Option<&NativeFileManager> {
        self.file_managers.get(session_id)
    }

    pub fn get_session(&self, session_id: &str) -> Option<&Session> {
        self.sessions.get(session_id)
    }

    pub fn get_all_sessions_meta(&self) -> Vec<SessionIdentity> {
        self.sessions.values().map(|s| s.identity.clone()).collect()
    }

    pub fn refresh_session_ttl(&mut self, session_id: &str) -> bool {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.last_heartbeat = std::time::Instant::now();
            true
        } else {
            false
        }
    }

    pub fn cleanup_stale_sessions(&mut self) {
        let now = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(30);
        
        let mut to_remove = Vec::new();
        for (id, session) in &self.sessions {
            if now.duration_since(session.last_heartbeat) > timeout {
                to_remove.push(id.clone());
            }
        }
        
        for id in to_remove {
            println!("RoBezy Cleanup: Removing stale session {}", id);
            self.unregister_session(&id);
        }
    }
}

// === WATCHER LOGIC ===
use notify::{Watcher, RecursiveMode, Event, EventKind};
use std::path::Path;

fn setup_watcher(folder_path: String, queue: Arc<Mutex<Vec<FileChange>>>, ignore_paths: Arc<Mutex<HashMap<String, std::time::Instant>>>) -> Option<RecommendedWatcher> {
    let (tx, rx) = std::sync::mpsc::channel();
    
    // Create watcher
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("RoBezy Watcher Error: Failed to create watcher: {}", e);
            return None;
        }
    };
    
    let path = Path::new(&folder_path);
    if let Err(e) = watcher.watch(path, RecursiveMode::Recursive) {
         eprintln!("RoBezy Watcher Error: Failed to watch path {}: {}", folder_path, e);
         return None;
    }
    
    // Spawn handler thread
    let folder_base = folder_path.clone(); // Clone for thread
    
    std::thread::spawn(move || {
        for res in rx {
            match res {
                Ok(Event { kind, paths, .. }) => {
                    // We care about Modify and Create (and Maybe Remove?)
                    // For now, let's focus on writes.
                    // Note: 'notify' can be spammy. Debouncing is ideal but let's do naive first.
                    match kind {
                        EventKind::Modify(_) | EventKind::Create(_) => {
                            for p in paths {
                                if p.is_file() {
                                    // Check extension
                                    if let Some(ext) = p.extension() {
                                        let ext_str = ext.to_string_lossy();
                                        if ext_str == "lua" || ext_str == "json" { // json for attributes?
                                            // Read content
                                            if let Ok(content) = std::fs::read_to_string(&p) {
                                                // Convert absolute path to relative path
                                                let relative = p.strip_prefix(&folder_base)
                                                    .unwrap_or(&p)
                                                    .to_string_lossy()
                                                    .to_string();
                                                
                                                let normalized_path = relative.replace("\\", "/");
                                                
                                                // CHECK IGNORE LIST (Anti-Loop)
                                                {
                                                    let mut ignores = ignore_paths.lock().unwrap();
                                                    if let Some(expiry) = ignores.get(&normalized_path) {
                                                        if std::time::Instant::now() < *expiry {
                                                            // println!("RoBezy Watcher: Ignoring self-write on {}", normalized_path);
                                                            continue;
                                                        } else {
                                                            // Expired
                                                            ignores.remove(&normalized_path);
                                                        }
                                                    }
                                                }
                                                
                                                // Heuristic ClassName from filename
                                                // .server.lua -> Script
                                                // .client.lua -> LocalScript
                                                // .lua -> ModuleScript
                                                let (name, class) = if relative.ends_with(".server.lua") {
                                                     ("Script", "Script")
                                                } else if relative.ends_with(".client.lua") {
                                                     ("LocalScript", "LocalScript")
                                                } else {
                                                     ("ModuleScript", "ModuleScript")
                                                };

                                                let change = FileChange {
                                                    change_type: "write".to_string(),
                                                    path: normalized_path.clone(),
                                                    content: Some(content),
                                                    is_script: true, // Assuming all Lua are scripts
                                                    guid: None, // We don't know the GUID from here easily
                                                    class_name: Some(class.to_string()),
                                                };
                                                
                                                // Push (Debounce check: peek last?)
                                                let mut q = queue.lock().unwrap();
                                                
                                                // DEDUPLICATION: Remove any existing pending change for this exact path
                                                q.retain(|c| c.path != normalized_path);
                                                
                                                q.push(change);
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        _ => {}
                    }
                },
                Err(e) => eprintln!("Watch error: {:?}", e),
            }
        }
    });
    
    Some(watcher)
}
