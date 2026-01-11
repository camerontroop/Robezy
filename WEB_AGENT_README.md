# RoBezy Studio Connector (V3) - System Architecture & API Guide

This document defines the integration interface for the **RoBezy Studio-First Architecture**. Unlike previous versions that wrapped Rojo, this system treats Roblox Studio as the primary source of truth, with a custom plugin handling bi-directional synchronization.

## 🏗 System Architecture

The system consists of three components:

1.  **Roblox Studio (The Client)**: Runs the `RoBezy` plugin. It manages the active game state, sends snapshots to the backend, and polls for code edits.
2.  **RoBezy Backend (The Server)**: A Rust-based local server (Port `3032` by default) that acts as the bridge. It manages sessions, file persistence, and bi-directional queuing.
3.  **Web Agent / App (The Observer)**: Connects to the Backend to view active sessions and "bind" an AI Agent to a specific project folder.

### Data Flow

*   **Read (Studio -> Disk)**:
    *   User edits script in Studio.
    *   Plugin detects change (via `Script.Changed`).
    *   Plugin POSTs data to `/robezy/sync`.
    *   Backend writes file to `~/Documents/RobloxProjects/<ProjectName>_<ID>/`.
*   **Write (Disk -> Studio)**:
    *   Agent/User edits file on disk.
    *   Backend Watcher detects change -> Queues update into `outbound_queue`.
    *   Plugin polls `/robezy/poll_changes` (1Hz).
    *   Plugin receives update and applies `Script.Source`.

---

## 🚀 Integration Workflow (For Web App)

To connect an AI Agent to a running Roblox session:

1.  **Discovery**:
    *   Poll `GET http://localhost:3032/robezy/sessions`.
    *   This returns a list of active Studio sessions (`place_name`, `session_id`, `bound_folder`).
2.  **Binding**:
    *   The Web App uses the `bound_folder` path from the session metadata.
    *   The Agent allows the user to "Select" this session.
3.  **Interaction**:
    *   **Reading Code**: The Agent reads files directly from the `bound_folder` on the local disk.
    *   **Writing Code**: The Agent writes files directly to the `bound_folder` on the local disk.
    *   **Context**: The Agent can read the file tree structure to understand the game (e.g., `ServerScriptService/Handler.server.lua`).

---

## 📡 API Reference

**Base URL**: `http://localhost:3032`
**CORS**: Enabled for all origins (`*`).

### 1. List Sessions
Get all currently connected Studio instances.

**Endpoint**: `GET /robezy/sessions`

**Response**:
```json
[
  {
    "place_id": 184123456,
    "place_name": "My RPG Game",
    "session_id": "550e8400-e29b-41d4-a716-446655440000",
    "project_id": "a1b2c3d4-...",
    "bound_folder": "/Users/user/Documents/RobloxProjects/MyRPGGame_a1b2c3d4"
  }
]
```
> **Usage**: Display this list to the user to let them choose which game to work on.

### 2. Connect (Internal / Plugin Only)
Used by the Plugin to register itself.

**Endpoint**: `POST /robezy/connect`
**Payload**:
```json
{
  "place_id": 123456,
  "place_name": "Baseplate",
  "session_id": "...",
  "project_id": "..."
}
```

### 3. Sync from Studio (Internal / Plugin Only)
Used by the Plugin to send script sources to disk.

**Endpoint**: `POST /robezy/sync`
**Payload**:
```json
{
  "session_id": "...",
  "changes": [
    {
      "change_type": "write",
      "path": "ServerScriptService.MyScript",
      "content": "print('Hello')",
      "is_script": true,
      "guid": "{UUID}",
      "class_name": "Script"
    }
  ]
}
```

### 4. Poll Changes (Internal / Plugin Only)
Used by the Plugin to ask for edits made by the Agent/User.

**Endpoint**: `GET /robezy/poll_changes?session_id=...`

**Response**:
```json
[
  {
    "change_type": "write",
    "path": "ServerScriptService.MyScript",
    "content": "print('Updated by Agent')",
    "is_script": true
### 5. Proxy Write (Web App -> Disk)
Used by the Web App to write code to the local file system. The Backend writes the file, and the Watcher automatically queues it for the Plugin.

**Endpoint**: `POST /robezy/proxy_write`
**Payload**:
```json
{
  "session_id": "...",
  "path": "ServerScriptService/MyScript.server.lua",
  "content": "print('Edited from Web App')"
}
```

---

## 📂 File System Structure

Projects are isolated to avoid overwriting files between different games.

**Location**: `~/Documents/RobloxProjects/`

**Naming Convention**:
*   Folder: `<PlaceName>_<ProjectID_Prefix>` (e.g., `MyGame_a1b2c3d4`)
*   Files:
    *   `Server Scripts` -> `.server.lua`
    *   `Local Scripts` -> `.client.lua`
    *   `Module Scripts` -> `.lua`

**Path Mapping**:
*   Roblox: `game.ServerScriptService.Managers.GameManager`
*   Disk: `.../ServerScriptService/Managers/GameManager.server.lua`

**Notes**:
*   The `Project ID` is persistent. It is stored in a `StringValue` named `RoBezyConfig` inside `ServerStorage` in the Roblox place file. This ensures that even if you rename the game, it maps to the same folder on disk.

---

## 🛠 Plugin Setup (V3)

1.  **Install & Enable**: Standard local plugin installation.
2.  **Open Panel**: Click the **RoBezy** button in the Plugins tab.
3.  **Status**:
    *   🔴 **Red**: Backend App disconnected.
    *   🟢 **Green**: Backend App Connected.
    *   **Button**: "CONNECT" initiates the session.
    *   **Auto-Sync**: Once connected, changes sync automatically.
4.  **Window Behavior**: The plugin window honors your open/closed preference. It will not auto-open on Play.
