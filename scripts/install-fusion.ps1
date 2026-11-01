<#
.SYNOPSIS
Install the locally built JCode integration and its pinned runtime on Windows.
.DESCRIPTION
Uses the standard per-user JCode directory and installer. No administrator rights
or source workspace are needed to run the installed program. Build first, then:
  .\scripts\install-fusion.ps1 -ArtifactExePath .\target\selfdev\jcode.exe
The runtime is prepared with npm ci --ignore-scripts in scripts/fusion-runtime.
#>
param(
    [Parameter(Mandatory = $true)][string]$ArtifactExePath,
    [string]$Version = 'v0.94.0-crush.3.zhCN',
    [string]$NodePath,
    [string]$RuntimePath = (Join-Path $PSScriptRoot 'fusion-runtime'),
    [string]$RuntimeDllDir,
    [switch]$SkipShortcut
)
$ErrorActionPreference = 'Stop'
$InstallRoot = Join-Path $env:LOCALAPPDATA 'jcode'
$ArtifactExePath = (Resolve-Path -LiteralPath $ArtifactExePath).Path
$RuntimePath = (Resolve-Path -LiteralPath $RuntimePath).Path
if (-not $NodePath) { $NodePath = (Get-Command node.exe -ErrorAction Stop).Source }
$NodePath = (Resolve-Path -LiteralPath $NodePath).Path
if ($Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+(?:[+.-][A-Za-z0-9.-]+)?$') {
    throw 'Invalid installation version'
}
if (-not (Test-Path -LiteralPath (Join-Path $RuntimePath 'node_modules\mcp-remote\dist\proxy.js'))) {
    throw 'Pinned runtime missing. Run npm ci --ignore-scripts in scripts/fusion-runtime first.'
}
$InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)
$NodeDir = Join-Path $InstallRoot 'tools\node'
$InstalledRuntime = Join-Path $InstallRoot 'tools\runtime'
New-Item -ItemType Directory -Path $NodeDir, $InstalledRuntime -Force | Out-Null
Copy-Item -LiteralPath $NodePath -Destination (Join-Path $NodeDir 'node.exe') -Force
foreach ($item in @('package.json', 'package-lock.json', 'configure.mjs', 'LICENSE-Node')) {
    Copy-Item -LiteralPath (Join-Path $RuntimePath $item) -Destination $InstalledRuntime -Force
}
# Robocopy preserves complete dependency trees and gives a clear failure code.
& robocopy.exe (Join-Path $RuntimePath 'node_modules') (Join-Path $InstalledRuntime 'node_modules') /E /NFL /NDL /NJH /NJS /NP | Out-Null
if ($LASTEXITCODE -ge 8) { throw "Runtime copy failed: robocopy exit $LASTEXITCODE" }
$VersionNumber = $Version.Substring(1)
$BinaryDirs = @((Join-Path $InstallRoot 'bin'), (Join-Path $InstallRoot 'builds\stable'), (Join-Path $InstallRoot "builds\versions\$VersionNumber"))
# GNU LLVM builds may import libunwind.dll. Carry runtime DLLs beside each copy.
if ($RuntimeDllDir) {
    $RuntimeDllDir = (Resolve-Path -LiteralPath $RuntimeDllDir).Path
    foreach ($dir in $BinaryDirs) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        foreach ($dll in Get-ChildItem -LiteralPath $RuntimeDllDir -Filter 'libunwind.dll' -File) {
            Copy-Item -LiteralPath $dll.FullName -Destination $dir -Force
        }
    }
}
$PreviousSkipReload = $env:JCODE_SKIP_SERVER_RELOAD
try {
    $env:JCODE_SKIP_SERVER_RELOAD = '1'
    & (Join-Path $PSScriptRoot 'install.ps1') -ArtifactExePath $ArtifactExePath -Version $Version -InstallDir (Join-Path $InstallRoot 'bin')
} finally {
    $env:JCODE_SKIP_SERVER_RELOAD = $PreviousSkipReload
}
$ConfigHome = if ($env:JCODE_HOME) { $env:JCODE_HOME } else { Join-Path $env:USERPROFILE '.jcode' }
& (Join-Path $NodeDir 'node.exe') (Join-Path $InstalledRuntime 'configure.mjs') (Join-Path $ConfigHome 'config.toml') $InstallRoot
if ($LASTEXITCODE -ne 0) { throw 'Configuration merge failed; original configuration was preserved or backed up.' }
Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\LICENSE') -Destination (Join-Path $InstallRoot 'LICENSE-JCode') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\FUSION.md') -Destination (Join-Path $InstallRoot 'README-Integration.md') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\README.zh-CN.md') -Destination (Join-Path $InstallRoot 'README.zh-CN.md') -Force
if (-not $SkipShortcut) {
    $MenuDir = Join-Path ([Environment]::GetFolderPath('Programs')) 'JCode'
    New-Item -ItemType Directory -Path $MenuDir -Force | Out-Null
    $Shell = New-Object -ComObject WScript.Shell
    $Shortcut = $Shell.CreateShortcut((Join-Path $MenuDir 'JCode.lnk'))
    $Shortcut.TargetPath = Join-Path $InstallRoot 'bin\jcode.exe'
    $Shortcut.WorkingDirectory = $env:USERPROFILE
    $Shortcut.Description = 'JCode 中文版：LSP 与远程 MCP'
    $Shortcut.Save()
}
& (Join-Path $InstallRoot 'bin\jcode.exe') --version
if ($LASTEXITCODE -ne 0) { throw 'Installed JCode failed its startup check' }
$InstalledExe = Join-Path $InstallRoot 'bin\jcode.exe'
$InstalledNode = Join-Path $NodeDir 'node.exe'
$Manifest = [ordered]@{
    version = $VersionNumber
    installed_at = [DateTime]::UtcNow.ToString('o')
    executable = $InstalledExe
    executable_sha256 = (Get-FileHash -LiteralPath $InstalledExe -Algorithm SHA256).Hash.ToLowerInvariant()
    node_version = (& $InstalledNode --version)
    node_sha256 = (Get-FileHash -LiteralPath $InstalledNode -Algorithm SHA256).Hash.ToLowerInvariant()
    runtime_lock_sha256 = (Get-FileHash -LiteralPath (Join-Path $InstalledRuntime 'package-lock.json') -Algorithm SHA256).Hash.ToLowerInvariant()
}
[System.IO.File]::WriteAllText((Join-Path $InstallRoot 'fusion-install.json'), ($Manifest | ConvertTo-Json), (New-Object System.Text.UTF8Encoding($false)))
Write-Host "Installed JCode integration: $(Join-Path $InstallRoot 'bin\jcode.exe')"
