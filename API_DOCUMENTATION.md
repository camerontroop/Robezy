# RoBezy API Guide (v1.0.7+)

**Version**: 1.0.7 (Lightweight Edition)
**Status**: Stable

This guide details how to integrate an AI Agent or Web IDE with RoBezy to read/write Roblox Studio code.

---

## ⚡️ Quick Summary

| Feature | Protocol | Endpoint | Purpose |
| :--- | :--- | :--- | :--- |
| **Discovery** | HTTP | `GET :3032/robezy/sessions` | Find active games and where their files are on disk. |
| **Events** | WebSocket | `ws://127.0.0.1:3031` | Listen for file changes and game tree structure. |
| **Files** | File System | N/A | Read/Write code directly to the `bound_folder` on disk. |

> **⚠️ CRITICAL**: Do NOT connect to port **3030**. That is for the internal plugin bridge only.

---

## 1. Discovery (Finding Games)

Before doing anything, your Agent needs to know *where* the game code is.

**Request:**
`GET http://127.0.0.1:3032/robezy/sessions`

**Response:**
```json
[
  {
    "place_name": "My RPG Game",
    "project_id": "a1b2c3d4-...",
    "session_id": "a1b2c3d4-...",
    "bound_folder": "/Users/cameron/Documents/RobloxProjects/My RPG Game_a1b2c3d4"
  }
]
```

*   **`bound_folder`**: This is the most important field. This folder is a **Mirror** of the Roblox game.
*   **`session_id`**: You will use this to filter WebSocket events.

---

## 2. The Game Tree (Snapshots)

RoBezy v1.0.7 uses a **Lightweight Snapshot** system. It does not send physics properties (Position, Color, etc.) to save bandwidth. It only sends the **Identity** of instances (Name, Class, Path) so the Agent knows what exists.

### Event: `workspace:fragment`
The game tree is split into chunks of ~2000 items. You must listen to the WebSocket (`:3031`) and accumulate these chunks.

**Payload:**
```json
{
  "type": "workspace:fragment",
  "session_id": "a1b2c3d4-...",  // Batch ID
  "chunk_index": 1,
  "items": [
    {
      "Name": "Workspace",
      "ClassName": "Workspace",
      "Path": "Workspace"
    },
    {
      "Name": "Baseplate",
      "ClassName": "Part",
      "Path": "Workspace.Baseplate"
    },
    {
      "Name": "GameManager",
      "ClassName": "ModuleScript",
      "Path": "ServerScriptService.GameManager"
    }
  ],
  "timestamp": 1700001234
}
```

### 💻 Code Example: Improving Context (JavaScript)

Since the tree comes in chunks, your Agent should buffer them.

```javascript
let treeBuffer = [];
let currentBatchId = null;
const DEBOUNCE_MS = 500;
let debounceTimer = null;

ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);

  if (msg.type === "workspace:fragment") {
    // 1. Check for new snapshot batch
    if (msg.session_id !== currentBatchId) {
      console.log("New Snapshot Started. Clearing Buffer.");
      treeBuffer = []; // Reset
      currentBatchId = msg.session_id;
    }

    // 2. Accumulate Items
    treeBuffer.push(...msg.items);

    // 3. Debounce Completion
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      console.log("Snapshot Complete!", treeBuffer.length, "items");
      updateAgentContext(treeBuffer);
    }, DEBOUNCE_MS);
  }
};
```

---

## 3. File Synchronization Logic (The Core)

This is the most critical part of the system. RoBezy uses a **Bi-Directional Sync** engine.

### A. The "Bound Folder"
When a session starts, RoBezy assigns a specific folder on your disk to that game (e.g., `.../My RPG Game_a1b2c3d4`).
*   **Safety**: This folder is a *mirror*. Deleting files here deletes them in Studio.
*   **mapping**: We map Studio Services to Folders.

### B. File Extension Rules (CRITICAL)
You **MUST** use the correct extension when creating files, or RoBezy won't know what class to create in Roblox.

| Extension | Roblox Class | Context |
| :--- | :--- | :--- |
| **`.server.lua`** | `Script` | Server-Side Logic (ServerScriptService) |
| **`.client.lua`** | `LocalScript` | Client-Side Logic (StarterPlayer) |
| **`.lua`** | `ModuleScript` | Shared Logic (ReplicatedStorage) |

> **Example**: To create a server script, you name it `GameManager.server.lua`. In Studio, it will appear as `GameManager` (Class: Script).

### C. Folder Structure Map
*   `ServerScriptService/` -> Contains `.server.lua` and `.lua` (Modules).
*   `ReplicatedStorage/` -> Contains `.lua` (Modules).
*   `StarterPlayer/StarterPlayerScripts/` -> Contains `.client.lua`.
*   `Workspace/` -> Can contain Scripts inside Parts (as folders).

### D. How to Sync (Step-by-Step)

#### 1. Creating a New Script
**Goal**: Create a new Server Script.
1.  **Agent**: Writes file `.../bound_folder/ServerScriptService/NewLogic.server.lua`
2.  **RoBezy**: Detects the file creation. Infers `Class = Script`.
3.  **Studio**: Automatically creates a `Script` instance named `NewLogic` inside `ServerScriptService`.
4.  **Result**: Code is live.

#### 2. Editing Code
**Goal**: Update existing code.
1.  **Agent**: Overwrites `.../bound_folder/ServerScriptService/NewLogic.server.lua` with new content.
2.  **RoBezy**: Detects change.
3.  **Studio**: Updates the `Source` property of `NewLogic`.

#### 3. Deleting
**Goal**: Remove a script.
1.  **Agent**: Deletes the file.
2.  **RoBezy**: Detects deletion.
3.  **Studio**: Destroys the Instance.

---

## 4. Real-Time Events (WebSocket)

The WebSocket at `ws://127.0.0.1:3031` is a **Firehose**. 
*   You do **NOT** need to send a "subscribe" message.
*   You do **NOT** need to Authenticate.
*   You **WILL** receive events for ALL connected sessions immediately upon connection.

*Filter messages by `session_id` if you support multiple connected projects.*
