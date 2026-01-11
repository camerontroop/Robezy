use std::path::PathBuf;
use tokio::fs;

fn get_roblox_plugins_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        // Primary: ~/Documents/Roblox/Plugins
        if let Some(mut doc_dir) = dirs::document_dir() {
            doc_dir.push("Roblox");
            doc_dir.push("Plugins");
            return Some(doc_dir);
        }
        
        // Fallback: ~/Library/Application Support/Roblox/Plugins
        dirs::data_dir().map(|p| p.join("Roblox/Plugins"))
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir().map(|p| p.join("Roblox/Plugins"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

// RENAMED from install_rojo_plugin
pub async fn install_plugins() -> Result<String, String> {
    let plugins_dir = get_roblox_plugins_dir()
        .ok_or("Could not find Roblox plugins directory")?;
    
    fs::create_dir_all(&plugins_dir).await
        .map_err(|e| format!("Failed to create plugins dir: {}", e))?;
    
    // Install LogListener
    // In a real build, we should bundle this file. For now, we'll write the raw XML content directly
    // or assume we can read it from resources. 
    // Simplified: Embed the XML string here to guarantee it exists without resource bundling complexity for this demo.
    let log_listener_xml = r#"<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">
	<Meta name="ExplicitAutoJoints">true</Meta>
	<External>null</External>
	<External>nil</External>
	<Item class="Script" referent="RBX0">
		<Properties>
			<Content name="LinkedSource"><null></null></Content>
			<int64 name="SourceAssetId">-1</int64>
			<BinaryString name="Tags"></BinaryString>
			<string name="Name">LogListener</string>
			<string name="ScriptGuid">{A1B2C3D4-E5F6-7890-1234-567890ABCDEF}</string>
			<ProtectedString name="Source"><![CDATA[local LogService = game:GetService("LogService")
local HttpService = game:GetService("HttpService")

local ScriptContext = game:GetService("ScriptContext")
local COMPANION_URL = "http://localhost:3030/logs"
local sessionId = tostring(os.time()) -- Init with startup time

local lastLogContent = ""
local lastLogTime = 0
local SAME_LOG_COOLDOWN = 2.0 -- Seconds to suppress duplicates

-- Generic Log Listener (Prints, Warnings, Info)
local function onMessageOut(message, messageType)
    -- Check for Sentinel
    if message == "--> SESSION START <--" then
        sessionId = tostring(os.time())
        print("Connector: New Session Started (" .. sessionId .. ")")
        return 
    end

    -- Skip errors here, handled by ScriptContext for better metadata
    if messageType == Enum.MessageType.MessageError then
        return
    end
    
    -- Deduplication
    if message == lastLogContent and (os.clock() - lastLogTime) < SAME_LOG_COOLDOWN then
        return
    end
    lastLogContent = message
    lastLogTime = os.clock()

    local typeStr = "info"
    if messageType == Enum.MessageType.MessageOutput then typeStr = "print"
    elseif messageType == Enum.MessageType.MessageInfo then typeStr = "info"
    elseif messageType == Enum.MessageType.MessageWarning then typeStr = "warning"
    end

    local payload = { 
        message = message, 
        type = typeStr, 
        timestamp = os.time(),
        sessionId = sessionId
    }

    pcall(function()
        HttpService:PostAsync(COMPANION_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
    end)
end

-- Rich Error Listener
local function onError(message, trace, script)
    local source = "Unknown"
    if script then
        source = script:GetFullName()
    end
    
    -- Deduplication (Message + Stack)
    local signature = message .. tostring(trace)
    if signature == lastLogContent and (os.clock() - lastLogTime) < SAME_LOG_COOLDOWN then
        return
    end
    lastLogContent = signature
    lastLogTime = os.clock()

    local payload = { 
        message = message, 
        type = "error", 
        timestamp = os.time(),
        sessionId = sessionId,
        source = source,
        stack = trace
    }

    pcall(function()
        HttpService:PostAsync(COMPANION_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
    end)
end

LogService.MessageOut:Connect(onMessageOut)
ScriptContext.Error:Connect(onError)
print("LogListener Active")
]]></ProtectedString>
		</Properties>
	</Item>
</roblox>"#;

    let listener_path = plugins_dir.join("LogListener.rbxmx");
    fs::write(&listener_path, log_listener_xml).await
        .map_err(|e| format!("Failed to write LogListener: {}", e))?;

    // Install WorkspaceListener
    let workspace_listener_xml = r##"<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">
	<Meta name="ExplicitAutoJoints">true</Meta>
	<External>null</External>
	<External>nil</External>
	<Item class="Script" referent="RBX0">
		<Properties>
			<Content name="LinkedSource"><null></null></Content>
			<int64 name="SourceAssetId">-1</int64>
			<BinaryString name="Tags"></BinaryString>
			<string name="Name">WorkspaceListener</string>
			<string name="ScriptGuid">{B2C3D4E5-F6G7-8901-2345-67890ABCDEF0}</string>
			<ProtectedString name="Source"><![CDATA[local HttpService = game:GetService("HttpService")
local Workspace = game:GetService("Workspace")
local RunService = game:GetService("RunService")

local COMPANION_URL = "http://localhost:3030/roblox/workspace" -- Updated URL for new endpoint
local DEBOUNCE_TIME = 1.0
local lastUpdate = 0
local updatePending = false

-- LIGHT SERIALIZATION (For UI Tree)
-- Minimal data: Name, ClassName, Children structure
local function serializeLight(inst)
    local children = {}
    for _, child in ipairs(inst:GetChildren()) do
        table.insert(children, serializeLight(child))
    end
    
    return {
        Name = inst.Name,
        ClassName = inst.ClassName,
        Children = children,
        Path = inst:GetFullName()
    }
end

-- FULL SERIALIZATION (For Agent Context)
-- Rich data including physics, logic, attributes
local function serializeFull(inst)
    local data = {}
    
    -- Basic
    data.Name = inst.Name
    data.ClassName = inst.ClassName
    data.Path = inst:GetFullName()
    
    -- Attributes
    data.Attributes = inst:GetAttributes()

    -- Physics (BasePart)
    if inst:IsA("BasePart") then
        data.Size = {inst.Size.X, inst.Size.Y, inst.Size.Z}
        data.Position = {inst.Position.X, inst.Position.Y, inst.Position.Z}
        data.Anchored = inst.Anchored
        data.CanCollide = inst.CanCollide
        data.Transparency = inst.Transparency
        data.Color = inst.Color:ToHex()
        data.Material = inst.Material.Name
    end
    
    -- Logic (Scripts/Values)
    if inst:IsA("Script") or inst:IsA("LocalScript") then
        data.Enabled = inst.Enabled
    end
    
    if string.find(inst.ClassName, "Value") then
        pcall(function() data.Value = inst.Value end)
    end
    
    local children = {}
    for _, child in ipairs(inst:GetChildren()) do
        table.insert(children, serializeFull(child))
    end
    data.Children = children

    return data
end

local function getServiceSnapshot(serviceName, serializer)
    local service = game:GetService(serviceName)
    if service then
        return serializer(service)
    end
    return nil
end

local function sendSnapshot()
    -- Guard: Only allow Server to send HTTP requests (prevents "Http requests can only be executed by game server" on Client)
    if not RunService:IsServer() then return end

    if os.clock() - lastUpdate < DEBOUNCE_TIME then
        if not updatePending then
            updatePending = true
            task.delay(DEBOUNCE_TIME, sendSnapshot)
        end
        return
    end

    lastUpdate = os.clock()
    updatePending = false

    local servicesToMap = {
        "Workspace", 
        "ReplicatedStorage", 
        "ServerScriptService", 
        "ServerStorage", 
        "StarterGui", 
        "StarterPack", 
        "StarterPlayer", 
        "Lighting",
        "SoundService"
    }

    -- 1. SEND LIGHT SNAPSHOT (Tree)
    local lightServices = {}
    for _, name in ipairs(servicesToMap) do
        lightServices[name] = getServiceSnapshot(name, serializeLight)
    end

    local lightPayload = {
        type = "workspace:tree",
        services = lightServices,
        timestamp = os.time()
    }
    local success, err = pcall(function()
        HttpService:PostAsync(COMPANION_URL, HttpService:JSONEncode(lightPayload), Enum.HttpContentType.ApplicationJson, false)
    end)
    if not success then
        print("Connector Error (Light Snapshot): " .. tostring(err))
    end
    
    -- 2. SEND FULL SNAPSHOT (Deep Context) - DISABLED (Too heavy, hits 1MB limit)
    -- local fullServices = {}
    -- for _, name in ipairs(servicesToMap) do
    --     fullServices[name] = getServiceSnapshot(name, serializeFull)
    -- end

    -- local fullPayload = {
    --     type = "workspace:full",
    --     services = fullServices,
    --     timestamp = os.time()
    -- }
    -- local successFull, errFull = pcall(function()
    --     HttpService:PostAsync(COMPANION_URL, HttpService:JSONEncode(fullPayload), Enum.HttpContentType.ApplicationJson, false)
    -- end)
    -- if not successFull then
    --     -- print("Connector Error (Full Snapshot): " .. tostring(errFull)) 
    -- end
    
    print("Connector: Game Snapshot Sent loop complete")
end

-- Listeners
for _, serviceName in ipairs({"Workspace", "ReplicatedStorage", "ServerScriptService", "ServerStorage", "StarterGui", "StarterPack", "StarterPlayer", "Lighting"}) do
    local service = game:GetService(serviceName)
    if service then
        service.DescendantAdded:Connect(sendSnapshot)
        service.DescendantRemoving:Connect(sendSnapshot)
    end
end

task.defer(sendSnapshot)
print("WorkspaceListener Active")
]]></ProtectedString>
		</Properties>
	</Item>
</roblox>"##;

    // [RESTORED] WorkspaceListener (User Request)
    let workspace_path = plugins_dir.join("WorkspaceListener.rbxmx");
    // Ensure we write it (restore behavior)
    fs::write(&workspace_path, workspace_listener_xml).await
        .map_err(|e| format!("Failed to write WorkspaceListener: {}", e))?;

    // [RESTORED] CommandListener (User Request)
    let command_listener_xml = r##"<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">
	<Meta name="ExplicitAutoJoints">true</Meta>
	<External>null</External>
	<External>nil</External>
	<Item class="Script" referent="RBX0">
		<Properties>
			<Content name="LinkedSource"><null></null></Content>
			<int64 name="SourceAssetId">-1</int64>
			<BinaryString name="Tags"></BinaryString>
			<string name="Name">CommandListener</string>
			<string name="ScriptGuid">{B2C3D4E5-F678-9012-3456-789ABCDEF012}</string>
			<ProtectedString name="Source"><![CDATA[local HttpService = game:GetService("HttpService")
local CollectionService = game:GetService("CollectionService")
local COMMAND_URL = "http://127.0.0.1:3030/roblox/commands"
local EXECUTION_URL = "http://127.0.0.1:3030/roblox/execution"
local POLL_INTERVAL = 0.5

-- Helper to find instance by path string
local function findInstanceByPath(path)
    local segments = {}
    for segment in string.gmatch(path, "[^%.]+") do
        table.insert(segments, segment)
    end
    if #segments == 0 then return nil end
    local current = game
    local serviceName = segments[1]
    local service = game:GetService(serviceName)
    if not service then
         if serviceName == "Workspace" then current = workspace 
         else return nil end
    else
         current = service
    end
    for i = 2, #segments do
        current = current:FindFirstChild(segments[i])
        if not current then return nil end
    end
    return current
end

local function serializeProperties(inst)
    local data = {}
    data.Name = inst.Name
    data.ClassName = inst.ClassName
    if inst:IsA("BasePart") then
        data.Size = {inst.Size.X, inst.Size.Y, inst.Size.Z}
        data.Position = {inst.Position.X, inst.Position.Y, inst.Position.Z}
        data.Anchored = inst.Anchored
        data.CanCollide = inst.CanCollide
        data.Transparency = inst.Transparency
        data.Color = inst.Color:ToHex()
        data.Material = inst.Material.Name
        data.Mass = inst:GetMass()
        data.CastShadow = inst.CastShadow
    end
    if inst:IsA("LuaSourceContainer") then
        local success, source = pcall(function() return inst.Source end)
        if success then
             data.SourcePreview = string.sub(source, 1, 500)
             data.LineCount = #string.split(source, "\n")
        end
    end
    if inst:IsA("Script") then
        data.Enabled = inst.Enabled
        data.RunContext = inst.RunContext.Name
    end
    data.Tags = CollectionService:GetTags(inst)
    data.Attributes = inst:GetAttributes()
    return data
end

local function executeCommand(cmd)
    if cmd.command_type == "query:instance" then
        local path = cmd.params.path
        local inst = findInstanceByPath(path)
        if inst then
            local props = serializeProperties(inst)
            local payload = { path = path, properties = props }
            pcall(function()
                HttpService:PostAsync(EXECUTION_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
            end)
        end
    end
end

local function pollCommands()
    local success, response = pcall(function()
        return HttpService:GetAsync(COMMAND_URL, false)
    end)
    if success then
        local valid, commands = pcall(function() return HttpService:JSONDecode(response) end)
        if valid and type(commands) == "table" then
            for _, cmd in ipairs(commands) do
                executeCommand(cmd)
            end
        end
    end
end

while true do
    pollCommands()
    task.wait(POLL_INTERVAL)
end
]]></ProtectedString>
		</Properties>
	</Item>
</roblox>"##;

    let command_path = plugins_dir.join("CommandListener.rbxmx");
    fs::write(&command_path, command_listener_xml).await
        .map_err(|e| format!("Failed to write CommandListener: {}", e))?;


    // Install RoBezyLoop
    let robezy_loop_xml = r##"<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">
	<Meta name="ExplicitAutoJoints">true</Meta>
	<External>null</External>
	<External>nil</External>
	<Item class="Script" referent="RBX0">
		<Properties>
			<Content name="LinkedSource"><null></null></Content>
			<int64 name="SourceAssetId">-1</int64>
			<BinaryString name="Tags"></BinaryString>
			<string name="Name">RoBezyLoop</string>
			<string name="ScriptGuid">{12345678-90AB-CDEF-1234-567890ABCDEF}</string>
			<ProtectedString name="Source"><![CDATA[local HttpService = game:GetService("HttpService")
local RunService = game:GetService("RunService")

-- ROBEZY V3 CONFIG
local ROBEZY_URL = "http://127.0.0.1:3032/robezy"
local POLL_URL = ROBEZY_URL .. "/poll_changes"
local CHECK_URL = ROBEZY_URL .. "/sessions"
local HEARTBEAT_URL = ROBEZY_URL .. "/heartbeat"
local UPLOAD_URL = ROBEZY_URL .. "/upload"
local CoreGui = game:GetService("CoreGui")

-- [REMOVED] Safety Wait (Causes hang in Edit mode)
-- if not game:IsLoaded() then
--     game.Loaded:Wait()
--     task.wait(1)
-- end

local ROBEZY_URL = "http://127.0.0.1:3032/robezy"
local CONNECT_URL = ROBEZY_URL .. "/connect"
local UPLOAD_URL = ROBEZY_URL .. "/upload"
local DISCONNECT_URL = ROBEZY_URL .. "/disconnect"
local HEARTBEAT_URL = ROBEZY_URL .. "/heartbeat"
local CHECK_URL = ROBEZY_URL .. "/sessions" -- Using this as heartbeat for now
local POLL_URL = ROBEZY_URL .. "/poll_changes" -- POLL INBOUND from FS

-- === THEME CONFIG ===
local Colors = {
    Background = Color3.fromRGB(32, 33, 36),
    Surface = Color3.fromRGB(45, 46, 50),
    Primary = Color3.fromRGB(0, 122, 255),
    PrimaryHover = Color3.fromRGB(0, 140, 255),
    Error = Color3.fromRGB(255, 69, 58),
    Success = Color3.fromRGB(48, 209, 88),
    TextPrimary = Color3.fromHex("FFFFFF"),
    TextSecondary = Color3.fromHex("CCCCCC"),
    Disabled = Color3.fromRGB(60, 60, 60)
}

-- === UI SETUP ===
local toolbar, button, widget, dockInfo

local success, err = pcall(function()
    toolbar = plugin:CreateToolbar("RoBezy Sync")
    if toolbar then
        button = toolbar:CreateButton("RoBezy", "Open RoBezy Sync", "rbxasset://textures/StudioToolbox/AssetConfig/package.png")
        button.ClickableWhenViewportHidden = true
    end

    dockInfo = DockWidgetPluginGuiInfo.new(
        Enum.InitialDockState.Right,
        false, false, 
        250, 400,
        200, 300
    )

    widget = plugin:CreateDockWidgetPluginGui("RoBezyWidget_V3", dockInfo)
    widget.Title = "RoBezy Sync"
end)

if not success then
    warn("RoBezy UI Setup Failed: " .. tostring(err))
    return
end

-- Main Frame
local mainFrame = Instance.new("Frame")
mainFrame.Name = "MainFrame"
mainFrame.Size = UDim2.fromScale(1, 1)
mainFrame.BackgroundColor3 = Colors.Background
mainFrame.BorderSizePixel = 0
mainFrame.Parent = widget

-- Header
local header = Instance.new("Frame")
header.Name = "Header"
header.Size = UDim2.new(1, 0, 0, 70)
header.BackgroundColor3 = Colors.Surface
header.BorderSizePixel = 0
header.Parent = mainFrame

local title = Instance.new("TextLabel")
title.Text = "RoBezy"
title.Font = Enum.Font.GothamBlack
title.TextSize = 28
title.TextColor3 = Colors.TextPrimary
title.Size = UDim2.fromScale(1, 1)
title.Position = UDim2.fromOffset(0, -8) -- Nudge
title.BackgroundTransparency = 1
title.Parent = header

local subtitle = Instance.new("TextLabel")
subtitle.Text = "SYNC STUDIO"
subtitle.Font = Enum.Font.GothamBold
subtitle.TextSize = 10
subtitle.TextColor3 = Colors.Primary
subtitle.Size = UDim2.fromScale(1, 0)
subtitle.Position = UDim2.new(0.5, 0, 0.75, 0)
subtitle.AnchorPoint = Vector2.new(0.5, 0.5)
subtitle.BackgroundTransparency = 1
subtitle.Parent = header

-- Status Section
local statusContainer = Instance.new("Frame")
statusContainer.Name = "StatusContainer"
statusContainer.Size = UDim2.fromScale(1, 0.2)
statusContainer.Position = UDim2.fromOffset(0, 80)
statusContainer.BackgroundTransparency = 1
statusContainer.Parent = mainFrame

local statusLabel = Instance.new("TextLabel")
statusLabel.Text = "COMPANION APP STATUS"
statusLabel.Font = Enum.Font.GothamBold
statusLabel.TextSize = 10
statusLabel.TextColor3 = Colors.TextSecondary
statusLabel.Size = UDim2.new(1, 0, 0, 20)
statusLabel.Position = UDim2.fromOffset(0, 0)
statusLabel.BackgroundTransparency = 1
statusLabel.Parent = statusContainer

local statusIndicator = Instance.new("Frame")
statusIndicator.Size = UDim2.fromOffset(12, 12)
statusIndicator.Position = UDim2.new(0.5, -70, 0.5, 10)
statusIndicator.AnchorPoint = Vector2.new(0.5, 0.5)
statusIndicator.BackgroundColor3 = Colors.Disabled
statusIndicator.Parent = statusContainer

local uiCornerDot = Instance.new("UICorner")
uiCornerDot.CornerRadius = UDim.new(1, 0)
uiCornerDot.Parent = statusIndicator

local statusText = Instance.new("TextLabel")
statusText.Text = "Searching..."
statusText.Font = Enum.Font.GothamMedium
statusText.TextSize = 14
statusText.TextColor3 = Colors.TextPrimary
statusText.Position = UDim2.new(0.5, 15, 0.5, 10)
statusText.AnchorPoint = Vector2.new(0.5, 0.5)
statusText.TextXAlignment = Enum.TextXAlignment.Left
statusText.BackgroundTransparency = 1
statusText.Parent = statusContainer

-- Connect Button
local connectBtn = Instance.new("TextButton")
connectBtn.Name = "ConnectBtn"
connectBtn.Size = UDim2.new(0.8, 0, 0, 50)
connectBtn.Position = UDim2.new(0.5, 0, 0.85, 0)
connectBtn.AnchorPoint = Vector2.new(0.5, 0.5)
connectBtn.BackgroundColor3 = Colors.Disabled
connectBtn.Text = "WAITING..."
connectBtn.Font = Enum.Font.GothamBold
connectBtn.TextSize = 18
connectBtn.TextColor3 = Colors.TextPrimary
connectBtn.AutoButtonColor = true
connectBtn.Parent = mainFrame

local btnCorner = Instance.new("UICorner")
btnCorner.CornerRadius = UDim.new(0, 8)
btnCorner.Parent = connectBtn

-- Info Footer
local footer = Instance.new("TextLabel")
footer.Text = "v3.0.0"
footer.Size = UDim2.new(1, 0, 0, 20)
footer.Position = UDim2.new(0, 0, 1, -25)
footer.BackgroundTransparency = 1
footer.TextColor3 = Colors.Disabled
footer.TextSize = 10
footer.Font = Enum.Font.Gotham
footer.Parent = mainFrame

-- === LOGIC STATE ===
local State = {
    Connected = false,
    SessionId = "",
    ProjectId = "",
    ApplyingChanges = false,
    AppDetected = false
}

-- DEBOUNCE CACHE
local LastWrittenContent = {}

-- FORWARD DECLARATIONS
local updateUI
local checkAppStatus

-- HELPER: ensureInstance
local function ensureInstance(fsPath, leafClass)
    fsPath = string.gsub(fsPath, "\\", "/")
    local segments = string.split(fsPath, "/")
    if #segments == 0 then return nil end
    
    local serviceName = segments[1]
    local success, current = pcall(function() return game:GetService(serviceName) end)
    if not success or not current then return nil end
    
    for i = 2, #segments do
        local nameStr = segments[i]
        local isLast = (i == #segments)
        
        if isLast then
             nameStr = string.gsub(nameStr, "%.server%.lua$", "")
             nameStr = string.gsub(nameStr, "%.client%.lua$", "")
             nameStr = string.gsub(nameStr, "%.lua$", "")
             nameStr = string.gsub(nameStr, "%.json$", "")
             if nameStr == "init" then return current end
        end
        
        local child = current:FindFirstChild(nameStr)
        if not child then
            if isLast then
                local classToCreate = leafClass or "ModuleScript"
                if classToCreate ~= "Script" and classToCreate ~= "LocalScript" and classToCreate ~= "ModuleScript" then
                    classToCreate = "ModuleScript"
                end
                child = Instance.new(classToCreate)
                child.Name = nameStr
                child.Parent = current
            else
                child = Instance.new("Folder")
                child.Name = nameStr
                child.Parent = current
            end
        end
        current = child
    end
    return current
end

local function syncLogic(scriptInstance)
    if not State.Connected then return end
    if State.ApplyingChanges then return end
    if LastWrittenContent[scriptInstance] == scriptInstance.Source then return end

    local SYNC_URL = ROBEZY_URL .. "/sync"
    local function getPath(instance)
        local path = instance.Name
        local callback = instance.Parent
        while callback and callback ~= game do
            path = callback.Name .. "/" .. path
            callback = callback.Parent
        end
        return path
    end
    
    local guid = scriptInstance:GetDebugId()
    local className = scriptInstance.ClassName
    local payload = {
        session_id = State.SessionId,
        changes = {{
            change_type = "write",
            path = getPath(scriptInstance),
            content = scriptInstance.Source,
            is_script = true,
            guid = guid,
            class_name = className
        }}
    }
    pcall(function()
        HttpService:PostAsync(SYNC_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
    end)
end

local function pollChanges()
    if not State.Connected then return end
    local url = POLL_URL .. "?session_id=" .. State.SessionId
    local success, response = pcall(function() return HttpService:GetAsync(url, true) end)
    
    if success then
        local valid, changes = pcall(function() return HttpService:JSONDecode(response) end)
        if valid and changes and #changes > 0 then
             print("RoBezy: Received " .. #changes .. " changes")
             State.ApplyingChanges = true
             for _, change in ipairs(changes) do
                  if change.change_type == "write" then
                       local inst = ensureInstance(change.path, change.class_name)
                       if inst and inst:IsA("LuaSourceContainer") and change.content then
                            print("RoBezy: Syncing " .. inst:GetFullName())
                            inst.Source = change.content
                            LastWrittenContent[inst] = change.content -- UPDATE DEBOUNCE
                       end
                  end
             end
             State.ApplyingChanges = false
        end
    end
end

-- WATCHERS
local function setupWatchers()
    local function watchScript(scriptInstance)
        if not scriptInstance:IsA("LuaSourceContainer") then return end
        scriptInstance.Changed:Connect(function(prop)
            if prop == "Source" then syncLogic(scriptInstance) end
        end)
        syncLogic(scriptInstance)
    end

    local services = {game.Workspace, game.ServerScriptService, game.ReplicatedStorage, game.ReplicatedFirst, game.StarterPlayer, game.StarterPack}
    for _, service in ipairs(services) do
        for _, desc in ipairs(service:GetDescendants()) do watchScript(desc) end
        service.DescendantAdded:Connect(watchScript)
    end
end

-- UI LOGIC
local function updateUI()
    -- Minimal UI update helper
    -- (Assuming UI objects exist globally in this scope or are created elsewhere)
end

local function checkAppStatus()
    local success, _ = pcall(function() return HttpService:GetAsync(CHECK_URL) end)
    local oldApp = State.AppDetected
    State.AppDetected = success
    if oldApp ~= State.AppDetected then updateUI() end
end

local function sendHeartbeat()
    if not State.Connected then return end
    pcall(function()
        local payload = { session_id = State.SessionId }
        HttpService:PostAsync(HEARTBEAT_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
    end)
end


-- MAIN LOOP
task.spawn(function()
    setupWatchers() -- Init watchers
    local tick = 0
    while true do
        checkAppStatus()
        pollChanges()
        tick = tick + 1
        if tick % 10 == 0 then sendHeartbeat() end
        task.wait(1)
    end
end)

print("RoBezy Professional UI Loaded (v3 + Two-Way Sync + Debounce Fixed)")
]]></ProtectedString>
		</Properties>
	</Item>
</roblox>"##;

    let command_path = plugins_dir.join("CommandListener.rbxmx");
    fs::write(&command_path, command_listener_xml).await
        .map_err(|e| format!("Failed to write CommandListener: {}", e))?;

    // Install RoBezyLoop
    let robezy_loop_xml = r##"<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">
	<Meta name="ExplicitAutoJoints">true</Meta>
	<External>null</External>
	<External>nil</External>
	<Item class="Script" referent="RBX0">
		<Properties>
			<Content name="LinkedSource"><null></null></Content>
			<int64 name="SourceAssetId">-1</int64>
			<BinaryString name="Tags"></BinaryString>
			<string name="Name">RoBezyLoop</string>
			<string name="ScriptGuid">{D4E5F6G7-8901-2345-6789-012345ABCDEF}</string>
			<ProtectedString name="Source"><![CDATA[local HttpService = game:GetService("HttpService")
local RunService = game:GetService("RunService")
local CoreGui = game:GetService("CoreGui")

-- [REMOVED] Safety Wait (Causes hang in Edit mode)
-- if not game:IsLoaded() then
--     game.Loaded:Wait()
--     task.wait(1)
-- end

local ROBEZY_URL = "http://127.0.0.1:3032/robezy"
local CONNECT_URL = ROBEZY_URL .. "/connect"
local UPLOAD_URL = ROBEZY_URL .. "/upload"
local DISCONNECT_URL = ROBEZY_URL .. "/disconnect"
local HEARTBEAT_URL = ROBEZY_URL .. "/heartbeat"
local CHECK_URL = ROBEZY_URL .. "/sessions" -- Using this as heartbeat for now
local POLL_URL = ROBEZY_URL .. "/poll_changes" -- POLL INBOUND from FS

-- === THEME CONFIG ===
local Colors = {
    Background = Color3.fromRGB(32, 33, 36),
    Surface = Color3.fromRGB(45, 46, 50),
    Primary = Color3.fromRGB(0, 122, 255),
    PrimaryHover = Color3.fromRGB(0, 140, 255),
    Error = Color3.fromRGB(255, 69, 58),
    Success = Color3.fromRGB(48, 209, 88),
    TextPrimary = Color3.fromHex("FFFFFF"),
    TextSecondary = Color3.fromHex("CCCCCC"),
    Disabled = Color3.fromRGB(60, 60, 60)
}

-- === UI SETUP ===
local toolbar, button, widget, dockInfo

local success, err = pcall(function()
    toolbar = plugin:CreateToolbar("RoBezy Sync")
    if toolbar then
        button = toolbar:CreateButton("RoBezy", "Open RoBezy Sync", "rbxasset://textures/StudioToolbox/AssetConfig/package.png")
        button.ClickableWhenViewportHidden = true
    end

    dockInfo = DockWidgetPluginGuiInfo.new(
        Enum.InitialDockState.Right,
        false, false, 
        250, 400,
        200, 300
    )

    widget = plugin:CreateDockWidgetPluginGui("RoBezyWidget_V3", dockInfo)
    widget.Title = "RoBezy Sync"
end)

if not success then
    warn("RoBezy UI Setup Failed: " .. tostring(err))
    return
end

-- Main Frame
local mainFrame = Instance.new("Frame")
mainFrame.Name = "MainFrame"
mainFrame.Size = UDim2.fromScale(1, 1)
mainFrame.BackgroundColor3 = Colors.Background
mainFrame.BorderSizePixel = 0
mainFrame.Parent = widget

-- Header
local header = Instance.new("Frame")
header.Name = "Header"
header.Size = UDim2.new(1, 0, 0, 70)
header.BackgroundColor3 = Colors.Surface
header.BorderSizePixel = 0
header.Parent = mainFrame

local title = Instance.new("TextLabel")
title.Text = "RoBezy"
title.Font = Enum.Font.GothamBlack
title.TextSize = 28
title.TextColor3 = Colors.TextPrimary
title.Size = UDim2.fromScale(1, 1)
title.Position = UDim2.fromOffset(0, -8) -- Nudge
title.BackgroundTransparency = 1
title.Parent = header

local subtitle = Instance.new("TextLabel")
subtitle.Text = "SYNC STUDIO"
subtitle.Font = Enum.Font.GothamBold
subtitle.TextSize = 10
subtitle.TextColor3 = Colors.Primary
subtitle.Size = UDim2.fromScale(1, 0)
subtitle.Position = UDim2.new(0.5, 0, 0.75, 0)
subtitle.AnchorPoint = Vector2.new(0.5, 0.5)
subtitle.BackgroundTransparency = 1
subtitle.Parent = header

-- Status Section
local statusContainer = Instance.new("Frame")
statusContainer.Name = "StatusContainer"
statusContainer.Size = UDim2.fromScale(1, 0.2)
statusContainer.Position = UDim2.fromOffset(0, 80)
statusContainer.BackgroundTransparency = 1
statusContainer.Parent = mainFrame

local statusLabel = Instance.new("TextLabel")
statusLabel.Text = "COMPANION APP STATUS"
statusLabel.Font = Enum.Font.GothamBold
statusLabel.TextSize = 10
statusLabel.TextColor3 = Colors.TextSecondary
statusLabel.Size = UDim2.new(1, 0, 0, 20)
statusLabel.Position = UDim2.fromOffset(0, 0)
statusLabel.BackgroundTransparency = 1
statusLabel.Parent = statusContainer

local statusIndicator = Instance.new("Frame")
statusIndicator.Size = UDim2.fromOffset(12, 12)
statusIndicator.Position = UDim2.new(0.5, -70, 0.5, 10)
statusIndicator.AnchorPoint = Vector2.new(0.5, 0.5)
statusIndicator.BackgroundColor3 = Colors.Disabled
statusIndicator.Parent = statusContainer

local uiCornerDot = Instance.new("UICorner")
uiCornerDot.CornerRadius = UDim.new(1, 0)
uiCornerDot.Parent = statusIndicator

local statusText = Instance.new("TextLabel")
statusText.Text = "Searching..."
statusText.Font = Enum.Font.GothamMedium
statusText.TextSize = 14
statusText.TextColor3 = Colors.TextPrimary
statusText.Position = UDim2.new(0.5, 15, 0.5, 10)
statusText.AnchorPoint = Vector2.new(0.5, 0.5)
statusText.TextXAlignment = Enum.TextXAlignment.Left
statusText.BackgroundTransparency = 1
statusText.Parent = statusContainer

-- Connect Button
local connectBtn = Instance.new("TextButton")
connectBtn.Name = "ConnectBtn"
connectBtn.Size = UDim2.new(0.8, 0, 0, 50)
connectBtn.Position = UDim2.new(0.5, 0, 0.85, 0)
connectBtn.AnchorPoint = Vector2.new(0.5, 0.5)
connectBtn.BackgroundColor3 = Colors.Disabled
connectBtn.Text = "WAITING..."
connectBtn.Font = Enum.Font.GothamBold
connectBtn.TextSize = 18
connectBtn.TextColor3 = Colors.TextPrimary
connectBtn.AutoButtonColor = true
connectBtn.Parent = mainFrame

local btnCorner = Instance.new("UICorner")
btnCorner.CornerRadius = UDim.new(0, 8)
btnCorner.Parent = connectBtn

-- Info Footer
local footer = Instance.new("TextLabel")
footer.Text = "v3.0.0"
footer.Size = UDim2.new(1, 0, 0, 20)
footer.Position = UDim2.new(0, 0, 1, -25)
footer.BackgroundTransparency = 1
footer.TextColor3 = Colors.Disabled
footer.TextSize = 10
footer.Font = Enum.Font.Gotham
footer.Parent = mainFrame

-- === LOGIC STATE ===
local State = {
    AppDetected = false,
    Connected = false,
    ApplyingChanges = false, -- Prevent loopback (Studio -> FS -> Studio)
    SessionId = "",
    ProjectId = ""
}

-- === FUNCTIONS ===

local function updateUI()
    if State.Connected then
        connectBtn.Text = "DISCONNECT"
        connectBtn.BackgroundColor3 = Colors.Error
        statusIndicator.BackgroundColor3 = Colors.Success
        statusText.Text = "Active Session"
        statusText.TextColor3 = Colors.Success
    elseif State.AppDetected then
        connectBtn.Text = "CONNECT"
        connectBtn.BackgroundColor3 = Colors.Primary
        statusIndicator.BackgroundColor3 = Colors.Success
        statusText.Text = "App Running"
        statusText.TextColor3 = Colors.TextPrimary
    else
        connectBtn.Text = "APP NOT DETECTED"
        connectBtn.BackgroundColor3 = Colors.Disabled
        statusIndicator.BackgroundColor3 = Colors.Error
        statusText.Text = "Not Found"
        statusText.TextColor3 = Colors.Error
    end
end

local function checkAppStatus()
    local success, _ = pcall(function()
        return HttpService:GetAsync(CHECK_URL)
    end)
    
    local oldApp = State.AppDetected
    State.AppDetected = success
    
    -- If App dies while connected, we effectively lose connection, but keep UI state until user notices or we implement watchdog
    if not State.AppDetected and State.Connected then
        -- Optional: Auto-disconnect visually warning
        -- State.Connected = false 
        -- print("RoBezy: Lost connection to app")
    end

    if oldApp ~= State.AppDetected then
        updateUI()
    end
end

local function syncLogic(scriptInstance)
    if not State.Connected then return end
    if State.ApplyingChanges then return end -- Don't sync back what we just received
    
    local SYNC_URL = ROBEZY_URL .. "/sync"
    
     local function getPath(instance)
        local path = instance.Name
        local callback = instance.Parent
        while callback and callback ~= game do
            path = callback.Name .. "." .. path
            callback = callback.Parent
        end
        return path
    end
    
    -- Use GetDebugId() for unique identification during session (handles same-named scripts)
    local guid = scriptInstance:GetDebugId()
    local className = scriptInstance.ClassName
    
    local payload = {
        session_id = State.SessionId,
        changes = {{
            change_type = "write",
            path = getPath(scriptInstance),
            content = scriptInstance.Source,
            is_script = true,
            guid = guid,
            class_name = className
        }}
    }
    pcall(function()
        HttpService:PostAsync(SYNC_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
    end)
end

-- === REVERSE SYNC (FS -> STUDIO) ===

local function resolveInstance(fsPath)
    -- fsPath example: "ServerScriptService/Folder/Script.server.lua"
    -- Normalize
    fsPath = string.gsub(fsPath, "\\", "/")
    
    local segments = string.split(fsPath, "/")
    if #segments == 0 then return nil end
    
    local serviceName = segments[1]
    local current = game:GetService(serviceName)
    if not current then return nil end
    
    for i = 2, #segments do
        local nameStr = segments[i]
        
        -- Clean extension if it's the last segment
        if i == #segments then
             nameStr = string.gsub(nameStr, "%.server%.lua$", "")
             nameStr = string.gsub(nameStr, "%.client%.lua$", "")
             nameStr = string.gsub(nameStr, "%.lua$", "")
             nameStr = string.gsub(nameStr, "%.json$", "")
        end
        
        -- Find child
        local nextObj = current:FindFirstChild(nameStr)
        if not nextObj then
             -- If not found, we can't edit it. Creating new files is complex (requiring class info).
             return nil 
        end
        current = nextObj
    end
    
    return current
end

local function ensureInstance(fsPath, leafClass)
    -- fsPath example: "ServerScriptService/Folder/Script.server.lua"
    fsPath = string.gsub(fsPath, "\\", "/")
    
    local segments = string.split(fsPath, "/")
    if #segments == 0 then return nil end
    
    local serviceName = segments[1]
    local success, current = pcall(function() return game:GetService(serviceName) end)
    if not success or not current then return nil end
    
    for i = 2, #segments do
        local nameStr = segments[i]
        local isLast = (i == #segments)
        
        -- Clean extension if it's the last segment
        if isLast then
             nameStr = string.gsub(nameStr, "%.server%.lua$", "")
             nameStr = string.gsub(nameStr, "%.client%.lua$", "")
             nameStr = string.gsub(nameStr, "%.lua$", "")
             nameStr = string.gsub(nameStr, "%.json$", "")
             -- Handle init.lua? (Rojo style)
             if nameStr == "init" then
                -- Special handling: init.lua usually means the parent folder IS the script.
                -- Use Parent as target. But Parent is 'current'.
                -- If we are at 'init.lua', we modify 'current', we don't create a child named 'init'.
                return current
             end
        end
        
        local child = current:FindFirstChild(nameStr)
        if not child then
            if isLast then
                -- Create the Script
                local classToCreate = leafClass or "ModuleScript"
                -- Validate class
                if classToCreate ~= "Script" and classToCreate ~= "LocalScript" and classToCreate ~= "ModuleScript" then
                    classToCreate = "ModuleScript"
                end
                
                child = Instance.new(classToCreate)
                child.Name = nameStr
                child.Parent = current
            else
                -- Create Intermediate Folder
                child = Instance.new("Folder")
                child.Name = nameStr
                child.Parent = current
            end
        end
        current = child
    end
    
    return current
end

local function pollChanges()
    if not State.Connected then return end
    
    local url = POLL_URL .. "?session_id=" .. State.SessionId
    local success, response = pcall(function()
        return HttpService:GetAsync(url, true) -- no cache
    end)
    
    if success then
        local valid, changes = pcall(function() return HttpService:JSONDecode(response) end)
        if valid and changes and #changes > 0 then
             print("RoBezy: Received " .. #changes .. " changes from polling")
             
             State.ApplyingChanges = true
             for _, change in ipairs(changes) do
                  if change.change_type == "write" then
                       -- Use ensureInstance instead of resolveInstance to CREATE if missing
                       local inst = ensureInstance(change.path, change.class_name)
                       if inst and inst:IsA("LuaSourceContainer") and change.content then
                            print("RoBezy: Syncing " .. inst:GetFullName())
                            inst.Source = change.content
                       else
                            print("RoBezy: Could not apply write to " .. change.path)
                       end
                  -- Handle Deletes? For now just Writes.
                  end
             end
             State.ApplyingChanges = false
        end
    end
end

local function setupWatchers()
    local function watchScript(scriptInstance)
        if not scriptInstance:IsA("LuaSourceContainer") then return end
        scriptInstance.Changed:Connect(function(prop)
            if prop == "Source" then
                syncLogic(scriptInstance)
            end
        end)
        syncLogic(scriptInstance)
    end

    local services = {game.Workspace, game.ServerScriptService, game.ReplicatedStorage, game.ReplicatedFirst, game.StarterPlayer, game.StarterPack}
    for _, service in ipairs(services) do
        for _, desc in ipairs(service:GetDescendants()) do
            watchScript(desc)
        end
        service.DescendantAdded:Connect(watchScript)
    end
end

local function gatherProjectFiles()
    local files = {}
    
    local function getPath(instance)
        local path = instance.Name
        local callback = instance.Parent
        while callback and callback ~= game do
            path = callback.Name .. "/" .. path
            callback = callback.Parent
        end
        return path
    end

    -- Add generic services
    local services = {
        game:GetService("Workspace"),
        game:GetService("ServerScriptService"),
        game:GetService("ReplicatedStorage"),
        game:GetService("ReplicatedFirst"),
        game:GetService("StarterPlayer"),
        game:GetService("StarterGui"),
        game:GetService("StarterPack"),
        game:GetService("ServerStorage"),
        game:GetService("Lighting")
    }
    
    for _, service in ipairs(services) do
        if service then
            for _, desc in ipairs(service:GetDescendants()) do
                 if desc:IsA("LuaSourceContainer") then
                     local path = getPath(desc)
                     local ext = ".lua"
                     
                     if desc:IsA("LocalScript") then
                        ext = ".client.lua"
                     elseif desc:IsA("ModuleScript") then
                        ext = ".lua"
                     elseif desc:IsA("Script") then
                        ext = ".server.lua"
                        -- Handle RunContext if needed, but .server.lua is safe default for Script
                        if desc.RunContext == Enum.RunContext.Client then
                            ext = ".client.lua"
                        end
                     end
                     
                     table.insert(files, {
                         path = path .. ext,
                         content = desc.Source
                     })
                 end
            end
        end
    end
    return files
end

local function onConnectClick()
    if not State.AppDetected then 
        print("RoBezy: Cannot connect, app not detected.")
        return 
    end
    
    if State.Connected then
        -- DISCONNECT
        pcall(function()
            local payload = { session_id = State.SessionId }
            HttpService:PostAsync(DISCONNECT_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
        end)
        
        State.Connected = false
        State.SessionId = ""
        updateUI()
        print("RoBezy: Disconnected")
        return
    end

    -- CONNECT
    connectBtn.Text = "CONNECTING..."
    
    -- 1. Session ID
    local sessionId = HttpService:GenerateGUID(false)
    State.SessionId = sessionId
    
    -- 2. Project ID
    local ServerStorage = game:GetService("ServerStorage")
    local configValue = ServerStorage:FindFirstChild("RoBezyConfig")
    local projectId = ""
    
    if configValue and configValue:IsA("StringValue") then
        local success, config = pcall(function() return HttpService:JSONDecode(configValue.Value) end)
        if success and config.id then projectId = config.id end
    end
    
    --[NEW] Don't generate ID locally if missing. Let Backend resolve it.
    if projectId == "" then
        projectId = nil
    end
    -- State.ProjectId = projectId -- Wait until confirmed

    -- 3. Gather & Chunk Files
    local allFiles = gatherProjectFiles()
    local filesToSend = allFiles
    local CHUNK_LIMIT_BYTES = 800000 -- 800KB safe limit (Roblox limit ~1MB)

    -- Estimate total size
    local totalSize = 0
    for _, f in ipairs(allFiles) do
        totalSize = totalSize + #f.content + #f.path + 50
    end
    
    if totalSize > CHUNK_LIMIT_BYTES then
        print("RoBezy: Project size (" .. math.floor(totalSize/1024) .. " KB) exceeds limit. Using chunked upload.")
        filesToSend = {} -- Send empty list in final connect
        
        local currentChunk = {}
        local currentChunkSize = 0
        local chunkCount = 0
        
        for _, f in ipairs(allFiles) do
            local itemSize = #f.content + #f.path + 50
            
            -- If adding this item exceeds limit, send current chunk
            if currentChunkSize + itemSize > CHUNK_LIMIT_BYTES and #currentChunk > 0 then
                chunkCount = chunkCount + 1
                print("RoBezy: Uploading chunk " .. chunkCount)
                local chunkPayload = { session_id = sessionId, files = currentChunk }
                local ok, err = pcall(function()
                     HttpService:PostAsync(UPLOAD_URL, HttpService:JSONEncode(chunkPayload), Enum.HttpContentType.ApplicationJson, false)
                end)
                if not ok then
                    warn("RoBezy Chunk Upload Failed: " .. tostring(err))
                    connectBtn.Text = "UPLOAD FAILED"
                    updateUI()
                    return
                end
                
                currentChunk = {}
                currentChunkSize = 0
            end
            
            table.insert(currentChunk, f)
            currentChunkSize = currentChunkSize + itemSize
        end
        
        -- Send last chunk
        if #currentChunk > 0 then
            chunkCount = chunkCount + 1
            print("RoBezy: Uploading chunk " .. chunkCount)
             local chunkPayload = { session_id = sessionId, files = currentChunk }
             local ok, err = pcall(function()
                  HttpService:PostAsync(UPLOAD_URL, HttpService:JSONEncode(chunkPayload), Enum.HttpContentType.ApplicationJson, false)
             end)
             if not ok then
                warn("RoBezy Chunk Upload Failed: " .. tostring(err))
                connectBtn.Text = "UPLOAD FAILED"
                updateUI()
                return
             end
        end
        print("RoBezy: Uploaded " .. chunkCount .. " chunks. Finalizing connection...")
    end

    local payload = {
        place_id = game.PlaceId,
        place_name = game.Name,
        session_id = sessionId,
        project_id = projectId,
        files = filesToSend
    }
    
    local success, response = pcall(function()
        return HttpService:PostAsync(CONNECT_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
    end)
    
    if success then
        print("RoBezy Connect Success")
        
        -- [NEW] Process Response (Get Stable ID)
        local data = nil
        pcall(function() data = HttpService:JSONDecode(response) end)
        
        if data and data.project_id then
            local serverId = data.project_id
            print("RoBezy: Server assigned Project ID: " .. serverId)
            State.ProjectId = serverId
            
            -- Persist to Config (so it saves with the place)
            if not configValue then
                configValue = Instance.new("StringValue")
                configValue.Name = "RoBezyConfig"
                configValue.Parent = ServerStorage
            end
            configValue.Value = HttpService:JSONEncode({ id = serverId, created = os.time() })
        end

        State.Connected = true
        setupWatchers()
        updateUI()
    else
        warn("RoBezy Connect Failed: " .. tostring(response))
        connectBtn.Text = "FAILED"
        task.wait(1)
        updateUI()
    end
end

-- === LISTENERS ===
connectBtn.Activated:Connect(onConnectClick)

-- FIXED: Button Toggle Logic
button.Click:Connect(function() 
    widget.Enabled = not widget.Enabled 
    button:SetActive(widget.Enabled)
end)

widget:GetPropertyChangedSignal("Enabled"):Connect(function()
    button:SetActive(widget.Enabled)
end)

-- Auto-Disconnect Safety
game:GetPropertyChangedSignal("Name"):Connect(function() 
    if State.Connected then State.Connected = false; updateUI() end 
end)
game:GetPropertyChangedSignal("PlaceId"):Connect(function() 
    if State.Connected then State.Connected = false; updateUI() end 
end)



local function sendHeartbeat()
    if not State.Connected then return end
    pcall(function()
        local payload = { session_id = State.SessionId }
        HttpService:PostAsync(HEARTBEAT_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
    end)
end

-- Heartbeat Loop
task.spawn(function()
    local tick = 0
    while true do
        checkAppStatus()
        pollChanges() -- POLL INBOUND from FS
        
        tick = tick + 1
        if tick % 10 == 0 then -- Every 10 seconds
             sendHeartbeat()
        end
        
        task.wait(1) -- Check every 1 second
    end
end)

print("RoBezy Professional UI Loaded (v3 + Two-Way Sync)")
]]></ProtectedString>
		</Properties>
	</Item>
</roblox>"##;

    let robezy_path = plugins_dir.join("RoBezyLoop.rbxmx");
    fs::write(&robezy_path, robezy_loop_xml).await
        .map_err(|e| format!("Failed to write RoBezyLoop: {}", e))?;
    
    Ok(format!("Plugins Updated: LogListener, WorkspaceListener, CommandListener, RoBezyLoop"))
}

pub async fn ensure_installed() -> Result<(), String> {
    let plugins_dir = get_roblox_plugins_dir()
        .ok_or("Could not find Roblox plugins directory")?;
    // Always update plugins to ensure latest UI fixes are applied
    println!("Updating plugins...");
    install_plugins().await.map(|_| ())
}
