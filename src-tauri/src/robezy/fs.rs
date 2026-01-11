use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::fs;

#[derive(Clone)]
pub struct NativeFileManager {
    pub root_dir: PathBuf,
    state: Arc<Mutex<FileManagerState>>,
}

struct FileManagerState {
    guid_to_path: HashMap<String, PathBuf>, // GUID -> Relative Path
    path_to_guid: HashMap<PathBuf, String>, // Relative Path -> GUID
}

impl NativeFileManager {
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
            state: Arc::new(Mutex::new(FileManagerState {
                guid_to_path: HashMap::new(),
                path_to_guid: HashMap::new(),
            })),
        }
    }

    /// Resolves the intended path for a GUID.
    /// If collision occurs, assigns a suffixed path (e.g. Script_1.lua).
    /// Returns the RELATIVE path.
    pub fn assign_path(&self, guid: &str, instance_path: &str, is_script: bool, class_name: Option<&str>) -> Option<PathBuf> {
        let mut state = self.state.lock().unwrap();

        // 1. If map already knows this GUID, return existing path
        if let Some(existing) = state.guid_to_path.get(guid) {
             // Return existing, assuming no rename logic yet
             // Check if we need to update extension if usage changes? Unlikely for same GUID.
             // But wait, if they change Script -> LocalScript, GUID stays same.
             // We SHOULD update the extension. 
             // But simplistic approach: Keep same file for now.
             // (Advanced: Detected ClassName change -> Rename file).
        }

        // 2. Calculate ideal relative path
        let parts: Vec<&str> = instance_path.split('.').collect();
        let mut base_path = PathBuf::new();
        for part in &parts {
             // Basic Sanitize
             let safe_part = part.chars()
                .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
                .collect::<String>();
             base_path.push(safe_part);
        }
        
        // 3. Apply Extension based on ClassName (Rojo Convention)
        if is_script {
            match class_name {
                Some("Script") => base_path.set_extension("server.lua"),
                Some("LocalScript") => base_path.set_extension("client.lua"),
                Some("ModuleScript") => base_path.set_extension("lua"),
                _ => base_path.set_extension("lua"), // Fallback
            };
        }
        
        // 4. Force Assignment (No Suffixes/Renaming Loop)
        // If multiple instances have the same name, they will fight over the same file.
        // This is preferable to infinite recursion/file explosion.
        
        let final_path = base_path;

        // Update maps (Steal ownership)
        if let Some(old_path) = state.guid_to_path.remove(guid) {
            if old_path != final_path {
                state.path_to_guid.remove(&old_path);
            }
        }
        
        state.path_to_guid.insert(final_path.clone(), guid.to_string());
        state.guid_to_path.insert(guid.to_string(), final_path.clone());
        
        Some(final_path)
    }

    /// Writes content to a file using the assigned path.
    pub async fn write_file_guid(&self, guid: &str, instance_path: &str, is_script: bool, class_name: Option<&str>, content: String) -> Result<PathBuf, String> {
        let relative_path = self.assign_path(guid, instance_path, is_script, class_name)
            .ok_or("Failed to buffer path")?;
            
        let final_path = self.root_dir.join(&relative_path);
        
        // Security check
        if !final_path.starts_with(&self.root_dir) {
            return Err("Access Denied".to_string());
        }

        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await
                .map_err(|e| format!("Dirs failed: {}", e))?;
        }

        fs::write(&final_path, content).await
            .map_err(|e| format!("Write failed: {}", e))?;
            
        Ok(relative_path)
    }
}
