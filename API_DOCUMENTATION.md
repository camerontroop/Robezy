# RoBezy Web Developer API Guide

This guide describes how to integrate a local Web App (Electron, React, Node.js) with the RoBezy Backend.

## 🏗 Architecture Overview

RoBezy runs two local servers to bridge the gap between specific Roblox Studio instances on your disk:

1.  **RoBezy V3 API (Port 3032)**: The primary REST API for session discovery and command/control.
2.  **Event Bus (Port 3031)**: A WebSocket server for listening to real-time changes from Studio.

### The "Hybrid" Workflow
Since RoBezy synchronizes Roblox Studio to a **local folder**, the most efficient workflow for an Agent/IDE is:
1.  **Discover**: Call the API to find where the game is stored on disk.
2.  **Read/Write**: Use standard File System (fs) operations to read/edit the code.
3.  **Listen**: Connect to the WebSocket to know *when* to re-read files (e.g. if the user changes something in Studio).

---

## 1. Discovery (Find the Game)

**Endpoint**: `GET http://127.0.0.1:3032/robezy/sessions`

Returns a list of all active Roblox Studio sessions currently connected to RoBezy.

**Response**:
```json
[
  {
    "place_id": 987654321,
    "place_name": "My Amazing RPG",
    "session_id": "a1b2c3d4-...",
    "project_id": "a1b2c3d4-...",
    "bound_folder": "/Users/name/Documents/RobloxProjects/My Amazing RPG_a1b2c3d4"
  }
]
```

*   **`bound_folder`**: This is the "Magic Key". This folder contains the **exact** mirroring of the Roblox game.
*   **`project_id`**: A stable, lifetime ID unique to this game file. It persists across restarts and renames.
*   **`session_id`**: (Legacy) This will now match `project_id` to ensure context consistency. Log events will use this ID.

---

## 2. Reading & Writing Code

### Option A: Direct File System (Recommended)
Since the `bound_folder` is on your local disk, you can use any standard library (node `fs`, python `open`, etc.) to interact with it.

*   **Read**: `fs.readFile(path.join(bound_folder, 'ServerScriptService', 'MyScript.server.lua'))`
*   **Write**: `fs.writeFile(...)`
    *   *RoBezy automatically detects file changes and pushes them to Roblox Studio.*

**File Extensions:**
*   `.server.lua` = `Script` (Server-side)
*   `.client.lua` = `LocalScript` (Client-side)
*   `.lua` = `ModuleScript` (Shared)

### Option B: Proxy API (If FS access is restricted)
If for some reason you cannot access the disk directly, you can ask RoBezy to write for you.

**Endpoint**: `POST http://127.0.0.1:3032/robezy/proxy_write`
```json
{
  "session_id": "...",
  "path": "ServerScriptService/MyScript.server.lua",
  "content": "print('Hello from API')"
}
```

---

## 3. Real-Time Events (WebSocket)

To keep your UI in perfect sync with Studio, connect to the Event Bus.

**URL**: `ws://127.0.0.1:3031`

### Incoming Messages (Server -> You)

#### `file:event` (File Changed)
Sent when a file updates (either from Studio or Disk).
```json
{
  "type": "file:event",
  "path": "ServerScriptService/MyScript.server.lua",
  "content": "print('New Content')", 
  "kind": "update" 
}
```
*   `kind`: `update` or `delete`.
*   Note: `content` might be `null` for deletes.

#### `project:sync` (Initial Load)
Sent when a new project connects. 
```json
{
  "type": "project:sync",
  "projectName": "MyGame_123",
  "files": [ ... ] 
}
```

#### `workspace:fragment` (Chunked Snapshot)
Sent when the game tree is too large for a single payload. The client should accumulate these fragments to build the full context.

```json
{
  "type": "workspace:fragment",
  "session_id": "1234567890",
  "chunk_index": 1,
  "items": [
    { "Name": "Part1", "ClassName": "Part", "Path": "Workspace.Part1", ... }
  ],
  "timestamp": 1234567890
}
```

#### `status`
Start a connection.
```json
{
  "type": "status",
  "connected": true
}
```

---

## 4. Full File Structure Map

If you need to know the entire tree of the game (folders, services, etc.), you can simply traverse the `bound_folder` directory structure.

*   `ServerScriptService/`
*   `ReplicatedStorage/`
*   `StarterPlayer/`
    *   `StarterPlayerScripts/`
    *   `StarterCharacterScripts/`
*   `Workspace/`

Roblox instances are mapped 1-to-1 with folders and files.

## Summary Checklist
1. [ ] Poll `GET :3032/robezy/sessions` to find the `bound_folder`.
2. [ ] Connect to `ws:// :3031` to listen for `file:event`.
3. [ ] Read/Write files in the `bound_folder`.
