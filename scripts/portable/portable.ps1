param(
    [string]$ProjectPath,
    [switch]$Stop,
    [switch]$PrepareOnly,
    [string[]]$JcodeArgs = @()
)
$ErrorActionPreference = 'Stop'
$taskRoot = [System.IO.Path]::GetFullPath($PSScriptRoot)
$taskExe = Join-Path $taskRoot 'bin\jcode.exe'
$taskNode = Join-Path $taskRoot 'tools\node\node.exe'
$taskAdapter = Join-Path $taskRoot 'tools\runtime\node_modules\mcp-remote\dist\proxy.js'
$taskData = Join-Path $taskRoot 'data'
$taskOldLocation = Get-Location
$taskEnvNames = @('JCODE_HOME', 'JCODE_RUNTIME_DIR', 'JCODE_SOCKET', 'JCODE_INSTALL_DIR', 'JCODE_NODE_BINARY', 'JCODE_MCP_REMOTE_BRIDGE', 'MCP_REMOTE_CONFIG_DIR', 'JCODE_NO_TELEMETRY', 'DO_NOT_TRACK', 'JCODE_DISABLE_CLAUDE_MCP', 'PATH')
$taskPreviousEnv = @{}
foreach ($taskName in $taskEnvNames) { $taskPreviousEnv[$taskName] = [Environment]::GetEnvironmentVariable($taskName, 'Process') }
$taskCode = 0
try {
    foreach ($taskFile in @($taskExe, $taskNode, $taskAdapter)) {
        if (-not (Test-Path -LiteralPath $taskFile -PathType Leaf)) { throw "Missing portable file: $taskFile" }
    }
    $taskSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $taskSha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $taskHostId = ([BitConverter]::ToString($taskSha.ComputeHash([Text.Encoding]::UTF8.GetBytes("$env:COMPUTERNAME|$taskSid")))).Replace('-', '').Substring(0, 16).ToLowerInvariant()
        $taskInstanceId = ([BitConverter]::ToString($taskSha.ComputeHash([Text.Encoding]::UTF8.GetBytes("$($taskRoot.ToLowerInvariant())|$taskSid")))).Replace('-', '').Substring(0, 20).ToLowerInvariant()
    } finally { $taskSha.Dispose() }
    $env:JCODE_HOME = $taskData
    $env:JCODE_RUNTIME_DIR = Join-Path $taskData "runtime\$taskHostId"
    $env:JCODE_SOCKET = "\\.\pipe\jcode-portable-$taskInstanceId"
    $env:JCODE_INSTALL_DIR = Join-Path $taskRoot 'bin'
    $env:JCODE_NODE_BINARY = $taskNode
    $env:JCODE_MCP_REMOTE_BRIDGE = $taskAdapter
    $env:MCP_REMOTE_CONFIG_DIR = Join-Path $taskData 'mcp-auth'
    $env:JCODE_NO_TELEMETRY = '1'
    $env:DO_NOT_TRACK = '1'
    $env:JCODE_DISABLE_CLAUDE_MCP = '1'
    $env:PATH = "$(Join-Path $taskRoot 'bin');$(Join-Path $taskRoot 'tools\node');$($taskPreviousEnv['PATH'])"
    New-Item -ItemType Directory -Path $taskData, $env:JCODE_RUNTIME_DIR -Force | Out-Null
    if ($Stop) {
        Set-Location -LiteralPath $taskRoot
        & $taskExe --no-update --no-selfdev server stop --force
        $taskCode = $LASTEXITCODE
    } else {
        & $taskNode (Join-Path $taskRoot 'tools\runtime\configure-portable.mjs') $taskRoot
        if ($LASTEXITCODE -ne 0) { throw 'Portable configuration preparation failed.' }
        if (-not $ProjectPath) { $ProjectPath = Join-Path $taskRoot 'Projects' }
        if (-not (Test-Path -LiteralPath $ProjectPath -PathType Container)) { throw "Project directory not found: $ProjectPath" }
        $taskProject = (Resolve-Path -LiteralPath $ProjectPath).Path
        if ($PrepareOnly) {
            [ordered]@{root=$taskRoot;executable=$taskExe;node=$taskNode;adapter=$taskAdapter;home=$taskData;runtime=$env:JCODE_RUNTIME_DIR;socket=$env:JCODE_SOCKET;project=$taskProject} | ConvertTo-Json -Compress
        } else {
            Set-Location -LiteralPath $taskProject
            & $taskExe --no-update --no-selfdev @JcodeArgs
            $taskCode = $LASTEXITCODE
        }
    }
} catch {
    [Console]::Error.WriteLine("JCode portable: $($_.Exception.Message)")
    $taskCode = 1
} finally {
    Set-Location -LiteralPath $taskOldLocation.Path
    foreach ($taskName in $taskEnvNames) { [Environment]::SetEnvironmentVariable($taskName, $taskPreviousEnv[$taskName], 'Process') }
}
exit $taskCode
