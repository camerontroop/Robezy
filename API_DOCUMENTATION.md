# RoBezy Web Developer API Guide (V2)

This guide describes how to integrate a local Web App (Electron, React, Node.js) with the RoBezy Backend (v1.0.6+).

## 🏗 Architecture Overview - The 3 Port System

RoBezy runs THREE local servers. Understanding which one to use is critical:

| Port | Name | Purpose |
| :--- | :--- | :--- |
| **3032** | **Public API** | **[USE THIS]** The main REST API for Agents to discover sessions and scan files. |
| **3031** | **Event Bus** | **[LISTEN HERE]** WebSocket server for real-time file updates and snapshots. |
| **3030** | **Internal Bridge** | **[IGNORE]** Private comms between Roblox Studio Plugin and RoBezy. |

### The "Hybrid" Workflow
Since RoBezy synchronizes Roblox Studio to a **local folder**, the most efficient workflow for an Agent/IDE is:
1.  **Discover**: Call `GET :3032/robezy/sessions` to act as a "Service Discovery".
2.  **Read/Write**: Use standard File System (fs) operations on the returned `bound_folder` path.
3.  **Context**: Accumulate `workspace:fragment` events from `ws:// :3031` to build a mental map of the game tree.

---

## 1. Discovery (Find the Game)

**Endpoint**: `GET http://127.0.0.1:3032/robezy/sessions`

Returns a list of all active Roblox Studio sessions.

**Response**:
```json
[
  {
    "place_id": 987654321,
    "place_name": "My Amazing RPG",
    "project_id": "a1b2c3d4-...",
    "session_id": "a1b2c3d4-...",
    "bound_folder": "/Users/name/Documents/RobloxProjects/My Amazing RPG_a1b2c3d4"
  }
]
```

*   **`bound_folder`**: This is the "Magic Key". This folder contains the **exact** mirroring of the Roblox game scripts.
*   **`project_id`**: A stable, lifetime ID unique to this game file. It persists across restarts and renames.
*   **`session_id`**: Matches `project_id`. Use this ID to correlate logs and events.

---

## 2. Real-Time Events (WebSocket)

**URL**: `ws://127.0.0.1:3031`

> **IMPORTANT**: There is **NO Handshake** or subscription API. Once connected, you will immediately start receiving global broadcasts for *all* active sessions. Be ready to filter by `session_id`.

### Incoming Messages (Server -> You)

#### `workspace:fragment` (Chunked Snapshot)
Sent when the game tree is scanned. Because game trees can be huge, they are sent in chunks (~2000 items each).

```json
{
  "type": "workspace:fragment",
  "session_id": "a1b2c3d4-...",
  "chunk_index": 1,
  "items": [
    { 
       "Name": "Part1", 
       "ClassName": "Part", 
       "Path": "Workspace.Part1"
    }
  ],
  "timestamp": 1700001234
}
```

> **Accumulation Strategy (Agent Logic):**
> 1.  Track the `session_id`. This acts as a "Batch ID".
> 2.  If the received `session_id` differs from your current buffer/cache, **clear your buffer** (it means a new full scan just started).
> 3.  Append `items` to your list.
> 4.  Wait for a short timeout (e.g. 500ms). If no new fragments arrive, the snapshot is complete.

#### `file:event` (File Changed)
Sent when a script is edited in Studio or on Disk.
```json
{
  "type": "file:event",
  "path": "ServerScriptService/MyScript.server.lua",
  "content": "print('New Content')", 
  "kind": "update" 
}
```

#### `status`
Hearbeat ping.
```json
{
  "type": "status",
  "connected": true
}
```

---

## 3. Reading & Writing Code

### Option A: Direct File System (Recommended)
Use `fs` based on the `bound_folder` path from Step 1.
*   **Read**: `fs.readFile()`
*   **Write**: `fs.writeFile()`
    *   *RoBezy automatically detects file changes and pushes them to Roblox Studio.*

### Option B: Proxy API (Restricted Environment)
If you cannot access the disk directly, use the Proxy.

**Endpoint**: `POST http://127.0.0.1:3032/robezy/proxy_write`
```json
{
  "session_id": "a1b2c3d4-...",
  "path": "ServerScriptService/MyScript.server.lua",
  "content": "print('Hello from API')"
}
```

---

## Summary Checklist for Agent Devs
1. [ ] **Connect**: `ws://127.0.0.1:3031` (No Handshake).
2. [ ] **Discover**: `GET http://127.0.0.1:3032/robezy/sessions`.
3. [ ] **Interact**: Read/Write files in `bound_folder` found in step 2.
4. [ ] **Context**: Listen for `workspace:fragment` on WS to build the game tree map.
