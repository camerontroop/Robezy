# RoBezy (Professional Studio Connector)

**RoBezy** is a next-generation file synchronization tool for Roblox Studio, designed effectively replacing legacy tools like Rojo for AI-assisted workflows. It enables seamless two-way synchronization between your local file system and Roblox Studio.

![RoBezy Banner](src/logo.png)

## 🚀 Features

*   **Two-Way Sync**: Edit scripts in VS Code (or by AI Agents) and see them update instantly in Studio. Edit in Studio and see them save to disk.
*   **Zero-Config Connection**: No `project.json` hell. Just click "Connect" and it works.
*   **Smart Folder Ownership**: safely manage multiple "Place 1" (unsaved) sessions simultaneously without file collisions.
*   **AI-Ready Architecture**: Built specifically to allow AI Coding Agents to read/write code directly on your disk, which is then reflected in-game.
*   **Cross-Platform**: Native apps for **Windows** and **macOS**.

## 📥 Installation

### Windows
1.  Download `RoBezy_..._setup.exe` from the [Releases Page](../../releases).
2.  Run the installer.
3.  Open RoBezy from your Start Menu.

### macOS
1.  Download `RoBezy_Mac_App.zip` from the [Releases Page](../../releases).
2.  Unzip the file.
3.  Drag `RoBezy.app` to your **Applications** folder.
4.  **Important**: Right-Click the app and select **Open** (to bypass safety warnings).

## 🛠 Usage

1.  **Open RoBezy**: Launch the desktop app. It will run quietly in the background (Core Service).
2.  **Open Roblox Studio**:
    *   Go to **Plugins** -> **Manage Plugins**.
    *   Find "RoBezy Sync" (it installs automatically).
    *   Click the **RoBezy** button in the toolbar.
3.  **Connect**:
    *   Click **CONNECT** in the plugin window.
    *   The tool will automatically create a sync folder in `Documents/RobloxProjects/<PlaceName>`.
4.  **Sync**:
    *   Any script you edit in Studio saves to that folder.
    *   Any file you edit (or create) in that folder appears in Studio.

## 🤖 For AI Agents & Developers

RoBezy exposes a local API for agents to inspect and modify the game state.
*   **API Endpoint**: `http://127.0.0.1:3032`
*   **Docs**: See [WEB_AGENT_README.md](WEB_AGENT_README.md) for full API documentation.

## 📂 Project Structure

Files are stored in `~/Documents/RobloxProjects/`.
*   `.server.lua` -> `Script`
*   `.client.lua` -> `LocalScript`
*   `.lua` -> `ModuleScript`

## License
MIT
