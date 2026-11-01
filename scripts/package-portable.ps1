<#
.SYNOPSIS
Create a self-contained Windows JCode directory without changing the installed app.
.EXAMPLE
.\scripts\package-portable.ps1 -ArtifactExePath .\target\selfdev\jcode.exe -DestinationPath E:\JCode-Portable
#>
param(
    [Parameter(Mandatory = $true)][string]$ArtifactExePath,
    [Parameter(Mandatory = $true)][string]$DestinationPath,
    [string]$RuntimeRoot = (Join-Path $env:LOCALAPPDATA 'jcode'),
    [string]$ProfilePath = (Join-Path $env:USERPROFILE '.jcode'),
    [string]$RuntimeDllDir,
    [switch]$SkipProfileMigration
)
$ErrorActionPreference = 'Stop'
$taskUtf8 = New-Object System.Text.UTF8Encoding($false)
$taskUtf8Bom = New-Object System.Text.UTF8Encoding($true)
$taskArtifact = (Resolve-Path -LiteralPath $ArtifactExePath).Path
$taskRuntime = (Resolve-Path -LiteralPath $RuntimeRoot).Path
$taskDestination = [IO.Path]::GetFullPath($DestinationPath)
if ($taskDestination -eq [IO.Path]::GetPathRoot($taskDestination)) { throw 'Choose a new package folder, not a drive root.' }
if (Test-Path -LiteralPath $taskDestination) { throw "Destination already exists; use a new folder: $taskDestination" }
$taskVersion = [regex]::Match([IO.File]::ReadAllText((Join-Path $PSScriptRoot '..\Cargo.toml')), '(?m)^version = "([^"]+)"').Groups[1].Value
if (-not $RuntimeDllDir) { $RuntimeDllDir = Split-Path -Parent $taskArtifact }
$taskDll = Join-Path $RuntimeDllDir 'libunwind.dll'
$taskSourceRuntime = Join-Path $taskRuntime 'tools\runtime'
$taskSourceNode = Join-Path $taskRuntime 'tools\node\node.exe'
foreach ($taskRequired in @($taskDll, $taskSourceNode, (Join-Path $taskSourceRuntime 'node_modules\mcp-remote\dist\proxy.js'), (Join-Path $taskSourceRuntime 'node_modules\typescript-language-server\lib\cli.mjs'))) {
    if (-not (Test-Path -LiteralPath $taskRequired -PathType Leaf)) { throw "Required runtime file missing: $taskRequired" }
}
$taskExpectedLock = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'fusion-runtime\package-lock.json') -Algorithm SHA256).Hash
if ((Get-FileHash -LiteralPath (Join-Path $taskSourceRuntime 'package-lock.json') -Algorithm SHA256).Hash -ne $taskExpectedLock) { throw 'Installed runtime does not match the pinned lock file.' }
function Copy-PortableTree([string]$Source, [string]$Target, [switch]$Profile) {
    $taskCopyArgs = @($Source, $Target, '/E', '/XJ', '/R:1', '/W:1', '/NFL', '/NDL', '/NJH', '/NJS', '/NP')
    if ($Profile) { $taskCopyArgs += @('/XF', '*.lock', '*.pid', '*.tmp') }
    & robocopy.exe @taskCopyArgs | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "Copy failed with robocopy exit $LASTEXITCODE" }
}
$taskBin = Join-Path $taskDestination 'bin'
$taskTools = Join-Path $taskDestination 'tools'
$taskData = Join-Path $taskDestination 'data'
New-Item -ItemType Directory -Path $taskBin, $taskTools, $taskData, (Join-Path $taskDestination 'Projects') -Force | Out-Null
Copy-Item -LiteralPath $taskArtifact -Destination (Join-Path $taskBin 'jcode.exe')
Copy-Item -LiteralPath $taskDll -Destination $taskBin
Copy-PortableTree (Join-Path $taskRuntime 'tools\node') (Join-Path $taskTools 'node')
Copy-PortableTree $taskSourceRuntime (Join-Path $taskTools 'runtime')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'fusion-runtime\configure-portable.mjs') -Destination (Join-Path $taskTools 'runtime\configure-portable.mjs')
[IO.File]::WriteAllText((Join-Path $taskDestination 'portable.ps1'), [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'portable\portable.ps1')), $taskUtf8Bom)
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'portable\README.zh-CN.md') -Destination (Join-Path $taskDestination '使用说明.md')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\LICENSE') -Destination (Join-Path $taskDestination 'LICENSE-JCode')
foreach ($taskEntry in @(@{Name='启动 JCode.cmd';Mode=''}, @{Name='停止便携版.cmd';Mode='-Stop'})) {
    $taskCmd = @"
@echo off
setlocal
chcp 65001 >nul
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0portable.ps1" $($taskEntry.Mode) %*
set "JCODE_PORTABLE_EXIT=%ERRORLEVEL%"
if not "%JCODE_PORTABLE_EXIT%"=="0" pause
exit /b %JCODE_PORTABLE_EXIT%
"@
    [IO.File]::WriteAllText((Join-Path $taskDestination $taskEntry.Name), ($taskCmd -replace "`r?`n", "`r`n"), $taskUtf8)
}
$taskCopied = New-Object 'System.Collections.Generic.List[string]'
$taskSkipped = New-Object 'System.Collections.Generic.List[string]'
if (-not $SkipProfileMigration -and (Test-Path -LiteralPath $ProfilePath -PathType Container)) {
    foreach ($taskItem in Get-ChildItem -LiteralPath $ProfilePath -Force) {
        if (($taskItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $taskItem.Name -match '^(active_pids|runtime|builds|hotkey|telemetry.*|servers\.json|last_focused_client_session|sessions-bak-prune\.stamp|config\.toml\.before-fusion-.*\.bak)$' -or $taskItem.Name -match '\.(lock|pid|tmp)$') {
            $taskSkipped.Add($taskItem.Name); continue
        }
        $taskTarget = Join-Path $taskData $taskItem.Name
        if ($taskItem.PSIsContainer) { Copy-PortableTree $taskItem.FullName $taskTarget -Profile }
        else { Copy-Item -LiteralPath $taskItem.FullName -Destination $taskTarget }
        $taskCopied.Add($taskItem.Name)
    }
    $taskAppConfig = Join-Path $env:APPDATA 'jcode'
    if (([IO.Path]::GetFullPath($ProfilePath) -eq [IO.Path]::GetFullPath((Join-Path $env:USERPROFILE '.jcode'))) -and (Test-Path -LiteralPath $taskAppConfig -PathType Container)) {
        Copy-PortableTree $taskAppConfig (Join-Path $taskData 'config\jcode') -Profile
        $taskCopied.Add('config/jcode')
    }
}
$taskPackageNode = Join-Path $taskTools 'node\node.exe'
$taskMigrationJson = & $taskPackageNode (Join-Path $taskTools 'runtime\configure-portable.mjs') $taskDestination --migrate
if ($LASTEXITCODE -ne 0) { throw 'Portable configuration migration failed.' }
$taskMigration = $taskMigrationJson | ConvertFrom-Json
$taskAccountStores = @(Get-ChildItem -LiteralPath $taskData -File -Force | Where-Object { $_.Name -match 'auth|credential|account|^\.env$|^env$' } | ForEach-Object { $_.Name })
$taskReport = [ordered]@{version=$taskVersion;created_at=[DateTime]::UtcNow.ToString('o');profile_migrated=(-not $SkipProfileMigration);copied_items=@($taskCopied.ToArray());skipped_items=@($taskSkipped.ToArray());account_store_files=$taskAccountStores;removed_mcp_servers=@($taskMigration.removed_mcp_servers);notes=@('Original computer data was retained.','Unsaved, expired or machine-bound logins require signing in on the destination computer.','Personal data and credentials are not included in the static hash manifest.')}
[IO.File]::WriteAllText((Join-Path $taskDestination 'migration-report.json'), ($taskReport | ConvertTo-Json -Depth 8), $taskUtf8)
$taskVersionOutput = & (Join-Path $taskBin 'jcode.exe') --version
if ($LASTEXITCODE -ne 0 -or $taskVersionOutput -notmatch [regex]::Escape("v$taskVersion")) { throw 'Portable executable version check failed.' }
$taskFiles = @(Get-ChildItem -LiteralPath $taskDestination -Recurse -File | Where-Object {
    $taskRelative = $_.FullName.Substring($taskDestination.Length + 1)
    $taskRelative -notmatch '^(data|Projects)[\\/]' -and $taskRelative -notin @('migration-report.json', 'portable-manifest.json')
} | ForEach-Object { [ordered]@{path=$_.FullName.Substring($taskDestination.Length + 1).Replace('\', '/');bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()} })
$taskManifest = [ordered]@{version=$taskVersion;created_at=[DateTime]::UtcNow.ToString('o');platform='windows-x64';binary_version=$taskVersionOutput;files=$taskFiles}
[IO.File]::WriteAllText((Join-Path $taskDestination 'portable-manifest.json'), ($taskManifest | ConvertTo-Json -Depth 6), $taskUtf8)
Write-Host "Portable package ready: $taskDestination"
Write-Host $taskVersionOutput
