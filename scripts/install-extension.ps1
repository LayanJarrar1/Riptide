<#
.SYNOPSIS
  Installs the Riptide browser extension into Chrome (or Edge), per user, no admin.

.DESCRIPTION
  1. Copies the built extension to a stable folder: %LOCALAPPDATA%\Riptide\extension\<browser>.
     The source is, in order: -Source, an "extension\<browser>" folder next to this script (portable
     zip / installer layout), "apps\extension\dist\<browser>" of the repository (built with npm when it
     is missing).
  2. Makes sure the Native Messaging manifest for "app.riptide.host" exists and points at the installed
     riptide-host.exe (HKCU only). If the Riptide app already registered itself correctly, nothing is
     touched; the app repairs the same values on every start.
  3. Prints the three steps, copies the folder path to the clipboard, opens chrome://extensions and shows
     the extension id you should see there.

  Chrome does not allow installing an unpacked extension silently (branded Chrome ignores
  --load-extension and policies need admin), so step 3 of the list is a click by you.

  Idempotent: run it again after every update of Riptide to refresh the files, then press the reload
  arrow on the extension card in chrome://extensions.

.PARAMETER Browser
  chrome (default) or edge.

.PARAMETER Source
  Folder with the built extension (contains manifest.json).

.PARAMETER HostExe
  Path of riptide-host.exe. Default: next to this script, %LOCALAPPDATA%\Riptide, the running app, or the
  repository's target\release.

.PARAMETER NoOpen
  Do not open the browser and do not touch the clipboard (scripts, tests).

.PARAMETER Uninstall
  Removes the copied extension folder and the manifest/registry value this script created. A registration
  made by the Riptide app itself is left alone. Remove the extension card in chrome://extensions by hand.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File .\install-extension.ps1
#>
[CmdletBinding()]
param(
    [ValidateSet('chrome', 'edge')]
    [string]$Browser = 'chrome',
    [string]$Source,
    [string]$HostExe,
    [switch]$NoOpen,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'

$HostName = 'app.riptide.host'
# Must match CHROMIUM_EXTENSION_ID in crates/riptide-host (derived from the manifest "key").
$ExpectedId = 'dcoeaodjhaibfchhcbfdkekpcacofnhm'

$Root = Join-Path $env:LOCALAPPDATA 'Riptide'
$Target = Join-Path (Join-Path $Root 'extension') $Browser
$ManifestDir = Join-Path $Root 'native-messaging'
$ManifestPath = Join-Path $ManifestDir "$HostName.chromium.json"
$RegKey = if ($Browser -eq 'edge') {
    "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$HostName"
} else {
    "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$HostName"
}
$ExtensionsUrl = if ($Browser -eq 'edge') { 'edge://extensions' } else { 'chrome://extensions' }

function Write-Step([string]$Text) { Write-Host "  $Text" }

function Get-ExtensionId([string]$ManifestFile) {
    # Chromium derives the id from the public key: SHA-256 of the DER bytes, first 16 bytes, hex digits
    # mapped 0-f -> a-p.
    $json = Get-Content -LiteralPath $ManifestFile -Raw -Encoding UTF8 | ConvertFrom-Json
    if (-not $json.key) { return $null }
    $der = [Convert]::FromBase64String([string]$json.key)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try { $hash = $sha.ComputeHash($der) } finally { $sha.Dispose() }
    $id = New-Object System.Text.StringBuilder
    for ($i = 0; $i -lt 16; $i++) {
        foreach ($nibble in @(($hash[$i] -shr 4), ($hash[$i] -band 15))) {
            [void]$id.Append([char](97 + $nibble))
        }
    }
    return $id.ToString()
}

function Find-Browser([string]$Which) {
    $exe = if ($Which -eq 'edge') { 'msedge.exe' } else { 'chrome.exe' }
    $appPaths = foreach ($hive in 'HKCU', 'HKLM') {
        $k = "${hive}:\Software\Microsoft\Windows\CurrentVersion\App Paths\$exe"
        if (Test-Path -LiteralPath $k) { (Get-ItemProperty -LiteralPath $k).'(default)' }
    }
    $candidates = @($appPaths)
    $pf = @($env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:LOCALAPPDATA) | Where-Object { $_ }
    foreach ($base in $pf) {
        if ($Which -eq 'edge') { $candidates += (Join-Path $base 'Microsoft\Edge\Application\msedge.exe') }
        else { $candidates += (Join-Path $base 'Google\Chrome\Application\chrome.exe') }
    }
    foreach ($c in $candidates) {
        if ($c -and (Test-Path -LiteralPath $c -PathType Leaf)) { return $c }
    }
    return $null
}

function Find-HostExe {
    $candidates = @()
    if ($HostExe) { $candidates += $HostExe }
    $candidates += (Join-Path $PSScriptRoot 'riptide-host.exe')
    $candidates += (Join-Path $Root 'riptide-host.exe')
    $running = Get-Process -Name 'riptide-desktop' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($running -and $running.Path) { $candidates += (Join-Path (Split-Path -Parent $running.Path) 'riptide-host.exe') }
    $candidates += (Join-Path $PSScriptRoot '..\target\release\riptide-host.exe')
    foreach ($c in $candidates) {
        if ($c -and (Test-Path -LiteralPath $c -PathType Leaf)) { return (Resolve-Path -LiteralPath $c).Path }
    }
    return $null
}

function Test-ManifestOk([string]$Path, [string]$Id) {
    # The registration is good when the manifest exists, allows our id and names an existing host exe.
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $false }
    try { $m = Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json } catch { return $false }
    if ($m.name -ne $HostName -or -not $m.path) { return $false }
    if (-not (Test-Path -LiteralPath $m.path -PathType Leaf)) { return $false }
    return (@($m.allowed_origins) -contains "chrome-extension://$Id/")
}

function Get-RegDefault([string]$Key) {
    if (-not (Test-Path -LiteralPath $Key)) { return $null }
    return (Get-ItemProperty -LiteralPath $Key).'(default)'
}

# ------------------------------------------------------------------------------------------ uninstall
if ($Uninstall) {
    Write-Host "Removing the Riptide $Browser extension files"
    if (Test-Path -LiteralPath $Target) {
        Remove-Item -LiteralPath $Target -Recurse -Force
        Write-Step "removed $Target"
    }
    $extParent = Split-Path -Parent $Target
    if ((Test-Path -LiteralPath $extParent) -and -not (Get-ChildItem -LiteralPath $extParent -Force)) {
        Remove-Item -LiteralPath $extParent -Force
    }
    # Only undo what this script wrote: a value that points at our own manifest in the shared folder.
    $current = Get-RegDefault $RegKey
    if ($current -and ([string]$current -ieq $ManifestPath)) {
        $other = if ($Browser -eq 'edge') { "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$HostName" } else { "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$HostName" }
        Remove-Item -LiteralPath $RegKey -Recurse -Force
        Write-Step "removed $RegKey"
        $otherValue = Get-RegDefault $other
        if (-not ($otherValue -and ([string]$otherValue -ieq $ManifestPath))) {
            if (Test-Path -LiteralPath $ManifestPath) { Remove-Item -LiteralPath $ManifestPath -Force; Write-Step "removed $ManifestPath" }
        }
    } else {
        Write-Step 'the browser bridge was registered by the Riptide app; left as it is'
    }
    Write-Host ''
    Write-Host "Last step: open $ExtensionsUrl and press Remove on the Riptide card."
    exit 0
}

# --------------------------------------------------------------------------------------------- install
Write-Host "Installing the Riptide extension for $Browser (no admin needed)"

# 1. find the built extension
$src = $null
$candidates = @()
if ($Source) { $candidates += $Source }
$candidates += (Join-Path $PSScriptRoot "extension\$Browser")
$repoDist = Join-Path $PSScriptRoot "..\apps\extension\dist\$Browser"
$candidates += $repoDist
foreach ($c in $candidates) {
    if ($c -and (Test-Path -LiteralPath (Join-Path $c 'manifest.json') -PathType Leaf)) { $src = (Resolve-Path -LiteralPath $c).Path; break }
}
if (-not $src) {
    $extProject = Join-Path $PSScriptRoot '..\apps\extension'
    if ((Test-Path -LiteralPath (Join-Path $extProject 'package.json')) -and (Get-Command npm -ErrorAction SilentlyContinue)) {
        Write-Step 'building the extension (npm ci && npm run build) ...'
        Push-Location $extProject
        try {
            if (-not (Test-Path -LiteralPath 'node_modules')) { & npm ci --no-audit --no-fund | Out-Null; if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' } }
            & npm run build | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'npm run build failed' }
        } finally { Pop-Location }
        if (Test-Path -LiteralPath (Join-Path $repoDist 'manifest.json')) { $src = (Resolve-Path -LiteralPath $repoDist).Path }
    }
}
if (-not $src) {
    throw "The built extension was not found. Pass -Source <folder with manifest.json>, or run the script from the Riptide folder / repository."
}

$sourceId = Get-ExtensionId (Join-Path $src 'manifest.json')
if (-not $sourceId) { throw "$src\manifest.json has no key; its id would change on every machine." }
if ($sourceId -ne $ExpectedId) {
    Write-Warning "The extension id of this build is $sourceId, but the Riptide host only trusts $ExpectedId unless integration.json lists it."
}

# 2. copy to the stable folder (mirror: remove files that no longer exist)
New-Item -ItemType Directory -Force -Path $Target | Out-Null
if ((Resolve-Path -LiteralPath $src).Path -ine (Resolve-Path -LiteralPath $Target).Path) {
    Get-ChildItem -LiteralPath $Target -Force | Remove-Item -Recurse -Force
    Copy-Item -Path (Join-Path $src '*') -Destination $Target -Recurse -Force
}
if (-not (Test-Path -LiteralPath (Join-Path $Target 'manifest.json'))) { throw "copy to $Target failed" }
Write-Step "extension files: $Target"

# 3. native messaging registration
$current = Get-RegDefault $RegKey
if ($current -and (Test-ManifestOk ([string]$current) $sourceId)) {
    Write-Step "browser bridge already registered: $current"
} else {
    $hostPath = Find-HostExe
    if (-not $hostPath) {
        Write-Warning 'riptide-host.exe was not found. Install/start Riptide once (it registers the bridge itself), or pass -HostExe.'
    } else {
        New-Item -ItemType Directory -Force -Path $ManifestDir | Out-Null
        $manifest = [ordered]@{
            name            = $HostName
            description     = 'Riptide download manager - browser bridge'
            path            = $hostPath
            type            = 'stdio'
            allowed_origins = @("chrome-extension://$sourceId/")
        }
        $json = $manifest | ConvertTo-Json -Depth 4
        # UTF-8 without BOM: Chrome refuses a manifest with a byte order mark.
        [System.IO.File]::WriteAllText($ManifestPath, $json, (New-Object System.Text.UTF8Encoding($false)))
        New-Item -Path $RegKey -Force | Out-Null
        Set-ItemProperty -LiteralPath $RegKey -Name '(default)' -Value $ManifestPath
        Write-Step "browser bridge registered: $ManifestPath -> $hostPath"
    }
}

# 4. tell the user
Write-Host ''
Write-Host "Expected extension id: $sourceId"
Write-Host ''
Write-Host 'Three steps in the browser:'
Write-Host "  1. On $ExtensionsUrl turn on 'Developer mode' (top right)."
Write-Host "  2. Press 'Load unpacked'."
Write-Host "  3. Pick this folder (its path is on the clipboard - paste it into the folder box):"
Write-Host "     $Target"
Write-Host "  The Riptide card must show ID $sourceId. Pin its icon from the puzzle-piece menu."
Write-Host ''
Write-Host 'After a Riptide update, run this script again and press the reload arrow on the card.'

if (-not $NoOpen) {
    try { Set-Clipboard -Value $Target } catch { Write-Warning 'could not copy the path to the clipboard' }
    $exe = Find-Browser $Browser
    if ($exe) { Start-Process -FilePath $exe -ArgumentList $ExtensionsUrl }
    else { Write-Warning "$Browser was not found; open $ExtensionsUrl yourself." }
}
