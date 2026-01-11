use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::io::Write;

pub fn get_projects_dir() -> PathBuf {
    let home = env::var("HOME").expect("HOME not set");
    Path::new(&home).join("RobloxProjects")
}

pub fn create_project(name: &str) -> std::io::Result<PathBuf> {
    let project_dir = get_projects_dir().join(name);
    
    // WIPEOUT: Remove existing directory to prevent stale files
    if project_dir.exists() {
        fs::remove_dir_all(&project_dir)?;
    }
    
    fs::create_dir_all(&project_dir)?;
    
    // Create standard folders
    fs::create_dir_all(project_dir.join("src/ServerScriptService"))?;
    fs::create_dir_all(project_dir.join("src/StarterPlayerScripts"))?;
    fs::create_dir_all(project_dir.join("src/ReplicatedStorage"))?;

    Ok(project_dir)
}

pub fn write_file(project_name: &str, relative_path: &str, content: &str) -> std::io::Result<()> {
    let project_dir = get_projects_dir().join(project_name);
    let full_path = project_dir.join(relative_path);

    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = fs::File::create(full_path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?; // Force flush to disk to trigger watchers immediately
    Ok(())
}

pub fn delete_file(project_name: &str, relative_path: &str) -> std::io::Result<()> {
    let project_dir = get_projects_dir().join(project_name);
    let full_path = project_dir.join(relative_path);
    if full_path.exists() {
        fs::remove_file(full_path)?;
    }
    Ok(())
}
