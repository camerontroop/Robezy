use warp::Filter;
use std::net::SocketAddr;

// Add broadcast support
use tokio::sync::broadcast;
use crate::server_ws::{InternalBroadcast, CommandQueue};

pub async fn start_server(log_tx: broadcast::Sender<InternalBroadcast>, command_queue: CommandQueue) {
    let status_route = warp::path("status")
        .map(|| {
            warp::reply::json(&serde_json::json!({
                "status": "ready",
                "version": "1.0.0"
            }))
        });

    let log_tx_filter = warp::any().map(move || log_tx.clone());

    let logs_route = warp::path("logs")
        .and(warp::post())
        .and(warp::body::json())
        .and(log_tx_filter.clone())
        .map(|body: serde_json::Value, tx: broadcast::Sender<InternalBroadcast>| {
            let _ = tx.send(InternalBroadcast::Log(body));
            warp::reply::json(&serde_json::json!({"status": "ok"}))
        });

    // POST /roblox/workspace
    let workspace_route = warp::path!("roblox" / "workspace")
        .and(warp::post())
        .and(warp::body::content_length_limit(1024 * 1024 * 50)) // 50MB limit
        .and(warp::body::json())
        .and(log_tx_filter.clone())
        .map(|body: serde_json::Value, tx: broadcast::Sender<InternalBroadcast>| {
            println!("HTTP: Received workspace snapshot"); // Debug log for user verification 
            // Check type
            let msg_type = body.get("type").and_then(|s| s.as_str()).unwrap_or("workspace:unknown");
            
            if msg_type == "workspace:tree" || msg_type == "workspace:full" {
                // Forward the whole body as a generic WorkspaceEvent
                // We'll define a new InternalBroadcast variant for this flexible event
                let _ = tx.send(InternalBroadcast::WorkspaceEvent(body));
            } else {
                 // Fallback for old style if any?
                 // Or just ignore.
                 // Old style was: { services: ..., hasTerrain: ... }
                 // Let's assume plugin is updated.
            }
            
            warp::reply::json(&serde_json::json!({"status": "ok"}))
        });

    // ROBLOX POLLING ENDPOINTS (Prefix /roblox/...)
    
    // GET /roblox/commands - Plugin polls this
    let queue_filter = warp::any().map(move || command_queue.clone());
    
    let commands_route = warp::path!("roblox" / "commands")
        .and(warp::get())
        .and(queue_filter.clone())
        .map(|queue: CommandQueue| {
            let mut cmds = Vec::new();
            if let Ok(mut q) = queue.lock() {
                // Drain all pending commands to send to plugin
                cmds = q.drain(..).collect();
            }
            warp::reply::json(&cmds)
        });

    // POST /roblox/execution - Plugin sends results here
    let execution_route = warp::path!("roblox" / "execution")
        .and(warp::post())
        .and(warp::body::json())
        .and(log_tx_filter.clone())
        .map(|body: serde_json::Value, tx: broadcast::Sender<InternalBroadcast>| {
             // Expecting { "path": "...", "properties": {...} }
             if let (Some(path), Some(props)) = (body.get("path").and_then(|s| s.as_str()), body.get("properties")) {
                 let _ = tx.send(InternalBroadcast::QueryResult { 
                     path: path.to_string(), 
                     properties: props.clone() 
                 });
             }
             warp::reply::json(&serde_json::json!({"status": "ok"}))
        });

    // CORS configuration
    let cors = warp::cors()
        .allow_any_origin()
        .allow_headers(vec!["Content-Type", "Accept", "User-Agent", "Sec-Fetch-Mode", "Referer", "Origin", "Access-Control-Request-Method", "Access-Control-Request-Headers", "Access-Control-Allow-Private-Network"])
        .allow_methods(vec!["GET", "POST", "OPTIONS"]);

    // Add PNA header manually as warp::cors might not set it automatically for the response
    let routes = status_route
        .or(logs_route)
        .or(workspace_route)
        .or(commands_route)
        .or(execution_route)
        .with(cors)
        .with(warp::reply::with::header("Access-Control-Allow-Private-Network", "true"));

    println!("HTTP server running on 3030");
    
    // Spawn IPv4
    let routes_v4 = routes.clone();
    tauri::async_runtime::spawn(async move {
        let addr: SocketAddr = ([0, 0, 0, 0], 3030).into();
        warp::serve(routes_v4).run(addr).await;
    });

    // Spawn IPv6
    tauri::async_runtime::spawn(async move {
        let addr: SocketAddr = ([0, 0, 0, 0, 0, 0, 0, 1], 3030).into();
        warp::serve(routes).run(addr).await;
    });
}
