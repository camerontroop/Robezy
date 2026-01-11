use notify::{Watcher, RecursiveMode, Result, RecommendedWatcher, Event, EventKind};
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;
use std::path::PathBuf;
use std::thread;
use std::sync::{Arc, Mutex};
use crate::fs_manager;

// Simple struct to hold the watcher and the channel to the WS server (not implemented fully here for brevity)
// In a real app we'd trigger a callback or send to a global channel.
pub struct ProjectWatcher {
    watcher: RecommendedWatcher,
}

impl ProjectWatcher {
    pub fn new<F>(project_name: String, on_change: F) -> Result<Self>
    where
        F: Fn(String, String) + Send + 'static,
    {
        let (tx, rx) = channel();
        
        // Notify watcher
        let mut watcher = notify::recommended_watcher(tx)?;

        let project_dir = fs_manager::get_projects_dir().join(&project_name);
        
        if project_dir.exists() {
             watcher.watch(&project_dir, RecursiveMode::Recursive)?;
        }

        thread::spawn(move || {
            for res in rx {
                match res {
                    Ok(Event { kind, paths, .. }) => {
                        // Filter interesting events
                        if let EventKind::Modify(_) | EventKind::Create(_) = kind {
                             for path in paths {
                                 // Simple file read and callback
                                 if path.is_file() {
                                     // Check if it's a Lua file or relevant
                                     if let Some(ext) = path.extension() {
                                         if ext == "lua" || ext == "json" { // simplified filter
                                              if let Ok(content) = std::fs::read_to_string(&path) {
                                                   // Convert absolute path to relative for the web app
                                                   // TODO: proper error handling and path stripping
                                                   let path_str = path.to_string_lossy().to_string(); 
                                                   on_change(path_str, content);
                                              }
                                         }
                                     }
                                 }
                             }
                        }
                    },
                    Err(e) => println!("watch error: {:?}", e),
                }
            }
        });

        Ok(Self { watcher })
    }
}
