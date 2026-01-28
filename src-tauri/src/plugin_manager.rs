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

pub async fn install_plugins() -> Result<String, String> {
    let plugins_dir = get_roblox_plugins_dir()
        .ok_or("Could not find Roblox plugins directory")?;
    
    fs::create_dir_all(&plugins_dir).await
        .map_err(|e| format!("Failed to create plugins dir: {}", e))?;
    
    // Install LogListener
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
local sessionId = tostring(os.time()) -- Default to timestamp
-- [NEW] Check immediately on load for stable ID
pcall(function()
    local sv = game:GetService("ServerStorage"):FindFirstChild("RoBezyConfig")
    if sv and sv:IsA("StringValue") then
        local data = HttpService:JSONDecode(sv.Value)
        if data.id then
            sessionId = data.id
        end
    end
end)

local lastLogContent = ""
local lastLogTime = 0
local SAME_LOG_COOLDOWN = 2.0 -- Seconds to suppress duplicates

-- Generic Log Listener (Prints, Warnings, Info)
local function onMessageOut(message, messageType)
    -- Check for Sentinel
    if message == "--> SESSION START <--" then
        sessionId = tostring(os.time())
        -- [NEW] Attempt to resolve stable Project ID from Storage
        pcall(function()
            local sv = game:GetService("ServerStorage"):FindFirstChild("RoBezyConfig")
            if sv and sv:IsA("StringValue") then
                local data = HttpService:JSONDecode(sv.Value)
                if data.id then
                    sessionId = data.id -- Override with Stable Project ID
                end
            end
        end)
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
			<ProtectedString name="Source"><![CDATA[local RunService = game:GetService("RunService")
local ServerScriptService = game:GetService("ServerScriptService")
local HttpService = game:GetService("HttpService")

-- CONSTANTS
local COMPANION_URL = "http://localhost:3030/roblox/workspace"
local DEBOUNCE_TIME = 2.0

-- STATE
local lastUpdate = 0
local updatePending = false

-- === PART 1: EDIT MODE LISTENER (Restored for Compatibility) ===
local function serializeInstance(inst)
    return {
        Name = inst.Name,
        ClassName = inst.ClassName,
        Path = inst:GetFullName()
    }
end

local function sendSnapshot()
    -- STRICT GUARD: Only sync in Edit Mode.
    -- If RunService is in Run Mode (Play Solo/Server), do NOT sync.
    if not RunService:IsEdit() then return end
    
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

    local buffer = {}
    local MAX_BUFFER = 2000 
    local chunkId = 0
    local sessionId = tostring(os.time()) 

     -- Try to find stable Session ID
    pcall(function()
        local sv = game:GetService("ServerStorage"):FindFirstChild("RoBezyConfig")
        if sv and sv:IsA("StringValue") then
            local data = HttpService:JSONDecode(sv.Value)
            if data.id then sessionId = data.id end
        end
    end)

    local function flush()
        if #buffer == 0 then return end
        chunkId = chunkId + 1
        
        local payload = {
            type = "workspace:fragment",
            session_id = sessionId,
            chunk_index = chunkId,
            items = buffer,
            timestamp = os.time()
        }
        
        pcall(function()
            HttpService:PostAsync(COMPANION_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
        end)
        
        buffer = {}
        task.wait(0.05) 
    end

    local function visit(inst)
        table.insert(buffer, serializeInstance(inst))
        if #buffer >= MAX_BUFFER then flush() end
    end

    local function traverse(inst)
        visit(inst)
        for _, child in ipairs(inst:GetChildren()) do traverse(child) end
    end

    print("RoBezy: Scanning Tree (Edit Mode)...") 
    for _, name in ipairs(servicesToMap) do
        local svc = game:GetService(name)
        if svc then traverse(svc) end
    end
    flush()
end

-- LISTENERS (The "As Is" Compatibility)
for _, serviceName in ipairs({"Workspace", "ReplicatedStorage", "ServerScriptService", "ServerStorage", "StarterGui", "StarterPack", "StarterPlayer", "Lighting"}) do
    local service = game:GetService(serviceName)
    if service then
        service.DescendantAdded:Connect(sendSnapshot)
        service.DescendantRemoving:Connect(sendSnapshot)
    end
end
task.defer(sendSnapshot)


-- CLEANUP LEGACY INJECTION (Revert to Edit Mode Truth)
local function cleanupRuntimeScript()
    local existing = ServerScriptService:FindFirstChild("RoBezy_Snapshot_Link")
    if existing then
        pcall(function()
            existing:Destroy()
            print("RoBezy: Cleaned up legacy Runtime Monitor")
        end)
    end
end

cleanupRuntimeScript()

print("WorkspaceListener Plugin Active (Edit Mode Only)")
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
			<string name="ScriptGuid">{D4E5F6G7-8901-2345-6789-012345ABCDEF}</string>
			<ProtectedString name="Source"><![CDATA[local HttpService = game:GetService("HttpService")
local RunService = game:GetService("RunService")
local CoreGui = game:GetService("CoreGui")

local ROBEZY_URL = "http://localhost:3032/robezy"
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
local toolbar, button, widget, dockInfo, mainFrame, header, statusContainer, statusIndicator, connectBtn, statusText, footer

-- SAFE UI CREATION (No Corners, No fancy styling that crashes)
local function createUI()
    if widget then return end -- Already created

    toolbar = plugin:CreateToolbar("RoBezy Sync")
    button = toolbar:CreateButton("RoBezy", "Open RoBezy Sync", "rbxasset://textures/StudioToolbox/AssetConfig/package.png")
    button.ClickableWhenViewportHidden = true
    
    dockInfo = DockWidgetPluginGuiInfo.new(
        Enum.InitialDockState.Right,
        false, false, 
        250, 400,
        200, 300
    )

    widget = plugin:CreateDockWidgetPluginGui("RoBezyWidget_V3", dockInfo)
    widget.Title = "RoBezy Sync"

    -- Main Frame
    mainFrame = Instance.new("Frame")
    mainFrame.Name = "MainFrame"
    mainFrame.Size = UDim2.fromScale(1, 1)
    mainFrame.BackgroundColor3 = Colors.Background
    mainFrame.BorderSizePixel = 0
    mainFrame.Parent = widget

    -- Header
    header = Instance.new("Frame")
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

    -- Status Section
    statusContainer = Instance.new("Frame")
    statusContainer.Name = "StatusContainer"
    statusContainer.Size = UDim2.fromScale(1, 0.2)
    statusContainer.Position = UDim2.fromOffset(0, 80)
    statusContainer.BackgroundTransparency = 1
    statusContainer.Parent = mainFrame

    -- INDICATOR (Box instead of circle to avoid UICorner crash)
    statusIndicator = Instance.new("Frame")
    statusIndicator.Size = UDim2.fromOffset(12, 12)
    statusIndicator.Position = UDim2.new(0.5, -70, 0.5, 10)
    statusIndicator.AnchorPoint = Vector2.new(0.5, 0.5)
    statusIndicator.BackgroundColor3 = Colors.Disabled
    statusIndicator.BorderSizePixel = 0
    statusIndicator.Parent = statusContainer

    statusText = Instance.new("TextLabel")
    statusText.Text = "Searching..."
    statusText.Font = Enum.Font.GothamMedium
    statusText.TextSize = 14
    statusText.TextColor3 = Colors.TextPrimary
    statusText.Position = UDim2.new(0.5, 15, 0.5, 10)
    statusText.AnchorPoint = Vector2.new(0.5, 0.5)
    statusText.TextXAlignment = Enum.TextXAlignment.Left
    statusText.BackgroundTransparency = 1
    statusText.Parent = statusContainer

    -- Connect Button (Box, no corner)
    connectBtn = Instance.new("TextButton")
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
    connectBtn.BorderSizePixel = 0
    connectBtn.Parent = mainFrame

    -- Info Footer
    footer = Instance.new("TextLabel")
    footer.Text = "v1.1.11"
    footer.Size = UDim2.new(1, 0, 0, 20)
    footer.Position = UDim2.new(0, 0, 1, -25)
    footer.BackgroundTransparency = 1
    footer.TextColor3 = Colors.Disabled
    footer.TextSize = 10
    footer.Font = Enum.Font.Gotham
    footer.Parent = mainFrame

    -- [CRITICAL FIX] TOOLBAR HANDLER
    button.Click:Connect(function()
        widget.Enabled = not widget.Enabled
    end)
end

-- Try creating UI, safely
local uiSuccess, uiErr = pcall(createUI)
if not uiSuccess then
    warn("RoBezy UI Setup Fatal Error: " .. tostring(uiErr))
end

-- === LOGIC STATE ===
local State = {
    AppDetected = false,
    Connected = false,
    ApplyingChanges = false,
    SessionId = "",
    ProjectId = ""
}

-- === FUNCTIONS ===

-- DIRTY STATE & POLL
local DirtyScripts = {} 
local LastWrittenContent = {}

local function countDirty()
    local c = 0
    for _, _ in pairs(DirtyScripts) do c = c + 1 end
    return c
end

local function updateUI()
    -- Guard against missing UI elements if setup failed
    if not connectBtn then return end

    if State.Connected then
        local dirtyCount = countDirty()
        
        if dirtyCount > 0 then
            connectBtn.Text = "PUSH CHANGES (" .. dirtyCount .. ")"
            connectBtn.BackgroundColor3 = Color3.fromRGB(255, 149, 0) 
        else
            connectBtn.Text = "DISCONNECT"
            connectBtn.BackgroundColor3 = Colors.Error
        end
        
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

-- FIX: Added missing checkAppStatus
local function checkAppStatus()
    local success, err = pcall(function()
        return HttpService:GetAsync(CHECK_URL)
    end)
    
    local oldApp = State.AppDetected
    State.AppDetected = success
    
    if not success and tick() % 10 < 1 then
         warn("RoBezy Connection Error: " .. tostring(err))
    end
    
    if oldApp ~= State.AppDetected then
        updateUI()
    end
end

local function syncLogic(scriptInstance)
    if not State.Connected then return end
    if State.ApplyingChanges then return end
    
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
    
     -- Mark clean + updating debounce logic to avoid loopback
    DirtyScripts[scriptInstance] = nil
    LastWrittenContent[scriptInstance] = scriptInstance.Source 
end


local function forceSyncAll()
    if not State.Connected then return end
    
    local dirtyCount = countDirty()
    if dirtyCount == 0 then return end
    
    if connectBtn then
        connectBtn.Text = "PUSHING..."
        connectBtn.BackgroundColor3 = Colors.PrimaryHover
    end
    task.wait(0.1)
    
    for scriptInst, _ in pairs(DirtyScripts) do
        syncLogic(scriptInst)
    end
    
    DirtyScripts = {}
    updateUI()
end

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
                            LastWrittenContent[inst] = change.content 
                            DirtyScripts[inst] = nil 
                       end
                  end
             end
             State.ApplyingChanges = false
             updateUI()
        end
    end
end

-- WATCHERS
local function setupWatchers()
    local function watchScript(scriptInstance)
        if not scriptInstance:IsA("LuaSourceContainer") then return end
        scriptInstance.Changed:Connect(function(prop)
            if prop == "Source" then 
                if State.ApplyingChanges then return end 
                if LastWrittenContent[scriptInstance] == scriptInstance.Source then return end 
                
                print("RoBezy: Detected Change in " .. scriptInstance.Name)
                DirtyScripts[scriptInstance] = true
                updateUI()
            end
        end)
    end
    local services = {
        game:GetService("Workspace"), 
        game:GetService("ServerScriptService"), 
        game:GetService("ReplicatedStorage"), 
        game:GetService("ReplicatedFirst"), 
        game:GetService("StarterPlayer"), 
        game:GetService("StarterPack"), 
        game:GetService("StarterGui"), 
        game:GetService("ServerStorage"), 
        game:GetService("Lighting")
    }
    for _, service in ipairs(services) do
        for _, desc in ipairs(service:GetDescendants()) do watchScript(desc) end
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
    local services = {game.Workspace, game.ServerScriptService, game.ReplicatedStorage, game.ReplicatedFirst, game.StarterPlayer, game.StarterGui, game.StarterPack, game.ServerStorage, game.Lighting}
    for _, service in ipairs(services) do
        for _, desc in ipairs(service:GetDescendants()) do
             if desc:IsA("LuaSourceContainer") and desc.Name ~= "RoBezy_Snapshot_Link" then
                 local path = getPath(desc)
                 local ext = ".lua"
                 if desc:IsA("LocalScript") then ext = ".client.lua"
                 elseif desc:IsA("Script") then ext = ".server.lua" end
                 table.insert(files, {path = path..ext, content = desc.Source})
             end
        end
    end
    return files
end

-- BUTTON HANDLER
if connectBtn then
    connectBtn.MouseButton1Click:Connect(function()
        if not State.AppDetected then return end
        
        if State.Connected then
            -- CHECK FOR DIRTY (Push Changes)
            local dirtyCount = countDirty()
            if dirtyCount > 0 then
                forceSyncAll()
                return
            end
            
            -- DISCONNECT
            pcall(function()
                local payload = { session_id = State.SessionId }
                HttpService:PostAsync(DISCONNECT_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
            end)
            State.Connected = false
            State.SessionId = ""
             updateUI()
        else
            -- CONNECT
            connectBtn.Text = "CONNECTING..."
            
            -- RESOLVE SESSION ID
            local ServerStorage = game:GetService("ServerStorage")
            local configValue = ServerStorage:FindFirstChild("RoBezyConfig")
            local storedId = nil
            if configValue and configValue:IsA("StringValue") then
                 local s, c = pcall(function() return HttpService:JSONDecode(configValue.Value) end)
                 if s and c.id then storedId = c.id end
            end
            local sessionId = storedId or HttpService:GenerateGUID(false)
            State.SessionId = sessionId
            local projectId = storedId
            
            -- GATHER FILES
            local allFiles = gatherProjectFiles()
            local filesToSend = allFiles
            
            local payload = {
                place_id = game.PlaceId,
                place_name = game.Name,
                session_id = sessionId,
                project_id = projectId,
                files = filesToSend
            }
        
            local success, resp = pcall(function()
                 return HttpService:PostAsync(CONNECT_URL, HttpService:JSONEncode(payload), Enum.HttpContentType.ApplicationJson, false)
            end)
            
            if success then
                local data = HttpService:JSONDecode(resp)
                State.SessionId = data.session_id
                State.ProjectId = data.project_id
                State.Connected = true
                
                 pcall(function()
                    local sv = game:GetService("ServerStorage"):FindFirstChild("RoBezyConfig")
                    if not sv then 
                        sv = Instance.new("StringValue")
                        sv.Name = "RoBezyConfig"
                        sv.Parent = game:GetService("ServerStorage")
                    end
                    sv.Value = HttpService:JSONEncode({id = State.SessionId})
                end)
                updateUI()
            else
                warn("RoBezy Connect Failed: " .. tostring(resp))
                connectBtn.Text = "FAILED"
                task.wait(1)
                updateUI()
            end
        end
    end)
end

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

-- MAIN LOOP
task.spawn(function()
    setupWatchers() 
    local tick = 0
    while true do
        checkAppStatus()
        pollChanges() 
        tick = tick + 1
        if tick % 10 == 0 then sendHeartbeat() end
        task.wait(1)
    end
end)

print("RoBezy Professional UI Loaded (v1.1.11 Safe+Toolbar)")
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
