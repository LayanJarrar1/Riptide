#Requires -Version 5.1
<#
.SYNOPSIS
    Throwaway silent install / upgrade / second install / uninstall cycle of the Riptide NSIS
    installer, asserting the browser-bridge registration after every step.

.DESCRIPTION
    What it proves (per user, no admin):
      1. fresh install   : the three HKCU Native Messaging keys and both manifests exist right after
                           setup, before the app ever ran; the manifests name the installed
                           riptide-host.exe and the extension ids from crates/riptide-host; the
                           extension folders are shipped; riptide-host.exe answers a ping from the
                           Chrome origin (argv) with "app not running" instead of "forbidden".
      2. upgrade         : a stale manifest / a missing key is repaired; user data (integration.json
                           with an extra extension id) survives.
      3. second install  : a second copy in another folder takes the registration over.
      4. uninstall #1    : the registration of the other copy is left alone.
      5. uninstall #2    : keys and manifests are gone, the manifest folder is gone.
      6. the real browser keys (Chrome / Edge / Firefox NativeMessagingHosts) never changed.

    Isolation: the registration is redirected to HKCU\Software\RiptideInstallerTest\<guid>
    (RIPTIDE_REGISTRY_ROOT) and a temp folder (RIPTIDE_INTEGRATION_DIR), the install dirs contain
    spaces and non-ASCII characters. The installer itself still writes the per-user uninstall
    entries and shortcuts of "Riptide", and would uninstall an existing Riptide it finds. So the
    script exports and removes the real HKCU\Software\riptide, the real Uninstall\Riptide key and the
    Riptide.lnk shortcuts first and restores them exactly at the end (absent stays absent), even
    when an assertion fails.

    It refuses to run (SKIPPED, exit 0) while riptide-desktop.exe is running: the installer would
    close the running app. Close Riptide first.

.PARAMETER Installer
    Path of Riptide_*_x64-setup.exe. Default: the newest one under target\release\bundle\nsis.

.PARAMETER KeepTemp
    Keep the temp folder (logs, exported registry files) after the run.
#>
[CmdletBinding()]
param(
    [string]$Installer,
    [switch]$KeepTemp
)

$ErrorActionPreference = 'Stop'
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}
$RepoRoot = Split-Path -Parent $PSScriptRoot

# ------------------------------------------------------------------------------ guards
$running = Get-Process -Name 'riptide-desktop' -ErrorAction SilentlyContinue
if ($running) {
    Write-Host 'SKIPPED: Riptide (riptide-desktop.exe) is running. The installer would close it and the' -ForegroundColor Yellow
    Write-Host '         single-instance lock stops a second copy. Close Riptide and run this script again.' -ForegroundColor Yellow
    exit 0
}
if (-not $Installer) {
    $found = Get-ChildItem -Path (Join-Path $RepoRoot 'target\release\bundle\nsis') -Filter 'Riptide_*_x64-setup.exe' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($found) { $Installer = $found.FullName }
}
if (-not $Installer -or -not (Test-Path -LiteralPath $Installer -PathType Leaf)) {
    throw 'No installer found. Build one (npm run bundle in apps/desktop) or pass -Installer <path>.'
}
$Installer = (Resolve-Path -LiteralPath $Installer).Path

# ids come from the Rust source of truth, not from this script
$libRs = Get-Content -LiteralPath (Join-Path $RepoRoot 'crates\riptide-host\src\lib.rs') -Raw -Encoding UTF8
$ChromiumId = [regex]::Match($libRs, 'CHROMIUM_EXTENSION_ID: &str = "([a-p]{32})"').Groups[1].Value
$FirefoxId = [regex]::Match($libRs, 'FIREFOX_EXTENSION_ID: &str = "([^"]+)"').Groups[1].Value
if (-not $ChromiumId -or -not $FirefoxId) { throw 'could not read the extension ids from crates/riptide-host/src/lib.rs' }

# ------------------------------------------------------------------------------ sandbox
$Guid = [guid]::NewGuid().ToString('N').Substring(0, 8)
$Temp = Join-Path ([IO.Path]::GetTempPath()) "riptide-installer-test-$Guid"
$OddName = 'Riptide Test ' + [char]0x00FC + [char]0x0645 + [char]0x0631  # "u-umlaut, Arabic meem, reh"
$DirA = Join-Path $Temp ($OddName + ' A')
$DirB = Join-Path $Temp ($OddName + ' B')
$IntegrationDir = Join-Path $Temp 'integration'
$RegRoot = "Software\RiptideInstallerTest\$Guid"
$Saved = Join-Path $Temp 'saved-state'
New-Item -ItemType Directory -Force -Path $Temp, $Saved, $IntegrationDir | Out-Null

$env:RIPTIDE_REGISTRY_ROOT = $RegRoot
$env:RIPTIDE_INTEGRATION_DIR = $IntegrationDir

$HostName = 'app.riptide.host'
$BrowserKeys = [ordered]@{
    chrome  = "Software\Google\Chrome\NativeMessagingHosts\$HostName"
    edge    = "Software\Microsoft\Edge\NativeMessagingHosts\$HostName"
    firefox = "Software\Mozilla\NativeMessagingHosts\$HostName"
}
# real keys the installer writes or reads (and that must come back exactly as they were)
$GuardedKeys = @(
    'Software\riptide',
    'Software\Microsoft\Windows\CurrentVersion\Uninstall\Riptide'
) + @($BrowserKeys.Values)
$Shortcuts = @(
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'Riptide.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'Riptide\Riptide.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Riptide.lnk')
)

$script:Failed = New-Object System.Collections.Generic.List[string]
$script:Passed = 0
function Assert([bool]$Condition, [string]$Message) {
    if ($Condition) { $script:Passed++; Write-Host "  ok   $Message" -ForegroundColor Green }
    else { $script:Failed.Add($Message); Write-Host "  FAIL $Message" -ForegroundColor Red }
}
function Step([string]$Text) { Write-Host ''; Write-Host "==> $Text" -ForegroundColor Cyan }

function RegPath([string]$Key) { "HKCU:\$Key" }
function Get-Default([string]$Key) {
    $p = RegPath $Key
    if (-not (Test-Path -LiteralPath $p)) { return $null }
    return (Get-ItemProperty -LiteralPath $p).'(default)'
}
function Snapshot([string]$Key) {
    # the whole subtree as text, to compare "before" and "after"
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("rt-snap-" + [guid]::NewGuid().ToString('N') + '.reg')
    & reg.exe export "HKCU\$Key" $tmp /y 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $tmp)) { return $null }
    $text = [IO.File]::ReadAllText($tmp)
    Remove-Item -LiteralPath $tmp -Force
    return $text
}

function Save-State {
    $i = 0
    foreach ($key in $GuardedKeys) {
        $i++
        $file = Join-Path $Saved ("key$i.reg")
        $exists = Test-Path -LiteralPath (RegPath $key)
        $script:SavedKeys += , @{ Key = $key; File = $file; Existed = $exists; Before = (Snapshot $key) }
        if ($exists) { & reg.exe export "HKCU\$key" $file /y | Out-Null }
    }
    $j = 0
    foreach ($lnk in $Shortcuts) {
        $j++
        if (Test-Path -LiteralPath $lnk) {
            $copy = Join-Path $Saved ("lnk$j.lnk")
            Copy-Item -LiteralPath $lnk -Destination $copy -Force
            $script:SavedLinks += , @{ Path = $lnk; Copy = $copy }
        }
    }
}
$script:SavedKeys = @()
$script:SavedLinks = @()

function Hide-RealInstall {
    # The installer detects a previous install through these two keys and would run ITS uninstaller.
    foreach ($key in 'Software\riptide', 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Riptide') {
        if (Test-Path -LiteralPath (RegPath $key)) { Remove-Item -LiteralPath (RegPath $key) -Recurse -Force }
    }
}

function Restore-State {
    # keys: delete whatever the run left, then import the export (absent stays absent)
    foreach ($entry in $script:SavedKeys) {
        $p = RegPath $entry.Key
        if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Recurse -Force -ErrorAction SilentlyContinue }
        if ($entry.Existed) { & reg.exe import $entry.File 2>&1 | Out-Null }
    }
    foreach ($lnk in $Shortcuts) {
        if (Test-Path -LiteralPath $lnk) { Remove-Item -LiteralPath $lnk -Force -ErrorAction SilentlyContinue }
    }
    foreach ($l in $script:SavedLinks) {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $l.Path) | Out-Null
        Copy-Item -LiteralPath $l.Copy -Destination $l.Path -Force
    }
    # the sandbox registry root
    $root = RegPath $RegRoot
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue }
    $parent = RegPath 'Software\RiptideInstallerTest'
    if ((Test-Path -LiteralPath $parent) -and -not (Get-ChildItem -LiteralPath $parent)) { Remove-Item -LiteralPath $parent -Force }
}

function Sandbox-Default([string]$Browser) { Get-Default ($RegRoot + '\' + $BrowserKeys[$Browser]) }

function Read-Manifest([string]$Path) {
    Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json
}

function Assert-Registered([string]$Dir, [string]$What) {
    $hostExe = Join-Path $Dir 'riptide-host.exe'
    $chromiumManifest = Join-Path $IntegrationDir "native-messaging\$HostName.chromium.json"
    $firefoxManifest = Join-Path $IntegrationDir "native-messaging\$HostName.firefox.json"
    Assert ((Sandbox-Default 'chrome') -ieq $chromiumManifest) "$What : Chrome key -> chromium manifest"
    Assert ((Sandbox-Default 'edge') -ieq $chromiumManifest) "$What : Edge key -> chromium manifest"
    Assert ((Sandbox-Default 'firefox') -ieq $firefoxManifest) "$What : Firefox key -> firefox manifest"
    Assert (Test-Path -LiteralPath $chromiumManifest) "$What : chromium manifest exists"
    Assert (Test-Path -LiteralPath $firefoxManifest) "$What : firefox manifest exists"
    if ((Test-Path -LiteralPath $chromiumManifest) -and (Test-Path -LiteralPath $firefoxManifest)) {
        $c = Read-Manifest $chromiumManifest
        $f = Read-Manifest $firefoxManifest
        Assert ($c.path -ieq $hostExe) "$What : chromium manifest names $hostExe"
        Assert ($f.path -ieq $hostExe) "$What : firefox manifest names the same host"
        Assert (@($c.allowed_origins) -contains "chrome-extension://$ChromiumId/") "$What : allowed_origins has the fixed extension id"
        Assert (@($f.allowed_extensions) -contains $FirefoxId) "$What : allowed_extensions has the Firefox id"
        Assert ($c.name -eq $HostName -and $c.type -eq 'stdio') "$What : manifest name and type"
    }
}

function Invoke-Installer([string[]]$ArgList, [string]$What) {
    $p = Start-Process -FilePath $Installer -ArgumentList $ArgList -Wait -PassThru
    Assert ($p.ExitCode -eq 0) "$What : installer exit code $($p.ExitCode)"
}

function Invoke-Uninstaller([string]$Dir, [string]$What) {
    $un = Join-Path $Dir 'uninstall.exe'
    Assert (Test-Path -LiteralPath $un) "$What : uninstall.exe exists"
    # _?= makes the uninstaller run in place and wait for it (it deletes itself otherwise)
    $p = Start-Process -FilePath $un -ArgumentList @('/S', "_?=$Dir") -Wait -PassThru
    Assert ($p.ExitCode -eq 0) "$What : uninstaller exit code $($p.ExitCode)"
    Remove-Item -LiteralPath $un -Force -ErrorAction SilentlyContinue
}

function Test-HostPing([string]$Dir, [string]$What) {
    # What Chrome does: start the host with the extension origin as argv, speak 4-byte-length JSON.
    $hostExe = Join-Path $Dir 'riptide-host.exe'
    # The frame goes in through a file redirect (cmd.exe), not through Process.StandardInput:
    # on Windows PowerShell 5.1 (.NET Framework) that stream starts with a UTF-8 byte order mark,
    # which the host rightly rejects as a 380 MB frame.
    $frame = Join-Path $Temp "ping-$([guid]::NewGuid().ToString('N')).bin"
    $reply_file = "$frame.out"
    $json = [Text.Encoding]::UTF8.GetBytes('{"id":7,"type":"ping"}')
    [IO.File]::WriteAllBytes($frame, ([byte[]][BitConverter]::GetBytes([int]$json.Length)) + $json)
    $saved = @{}
    foreach ($k in 'RIPTIDE_HOST_NO_LAUNCH', 'RIPTIDE_IPC_ENDPOINT', 'RIPTIDE_HOST_DATA_DIR') { $saved[$k] = [Environment]::GetEnvironmentVariable($k, 'Process') }
    try {
        $env:RIPTIDE_HOST_NO_LAUNCH = '1'
        $env:RIPTIDE_IPC_ENDPOINT = "\\.\pipe\riptide-installer-test-$Guid-none"
        $env:RIPTIDE_HOST_DATA_DIR = $IntegrationDir
        $cmdline = '/c ""' + $hostExe + '" "chrome-extension://' + $ChromiumId + '/" < "' + $frame + '" > "' + $reply_file + '""'
        $proc = Start-Process -FilePath "$env:ComSpec" -ArgumentList $cmdline -WindowStyle Hidden -PassThru
        if (-not $proc.WaitForExit(15000)) { $proc.Kill() }
        $bytes = if (Test-Path -LiteralPath $reply_file) { [IO.File]::ReadAllBytes($reply_file) } else { [byte[]]@() }
        $reply = ''
        if ($bytes.Length -ge 4) {
            $len = [BitConverter]::ToInt32($bytes, 0)
            $take = [Math]::Min($len, $bytes.Length - 4)
            $reply = [Text.Encoding]::UTF8.GetString($bytes, 4, $take)
        }
        Assert ($reply -match '"id"\s*:\s*7') "$What : host answers from a path with spaces and non-ASCII ($reply)"
        Assert ($reply -match 'not running') "$What : Chrome origin accepted, app reported as not running"
        Assert ($reply -notmatch 'not allowed') "$What : host did not refuse the extension"
    } finally {
        foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k], 'Process') }
        Remove-Item -LiteralPath $frame, $reply_file -Force -ErrorAction SilentlyContinue
    }
}

# ------------------------------------------------------------------------------ run
Write-Host "Installer : $Installer"
Write-Host "Install A : $DirA"
Write-Host "Install B : $DirB"
Write-Host "Registry  : HKCU\$RegRoot (sandbox)"
Write-Host "Temp      : $Temp"

try {
    Save-State
    Hide-RealInstall

    Step '1. fresh install (silent, per user, no admin)'
    Invoke-Installer @('/S', "/D=$DirA") 'fresh install'
    Assert (Test-Path -LiteralPath (Join-Path $DirA 'riptide-desktop.exe')) 'riptide-desktop.exe installed'
    Assert (Test-Path -LiteralPath (Join-Path $DirA 'riptide-host.exe')) 'riptide-host.exe installed'
    Assert (Test-Path -LiteralPath (Join-Path $DirA 'extension\chrome\manifest.json')) 'extension\chrome shipped'
    Assert (Test-Path -LiteralPath (Join-Path $DirA 'extension\edge\manifest.json')) 'extension\edge shipped'
    Assert (Test-Path -LiteralPath (Join-Path $DirA 'install-extension.ps1')) 'install-extension.ps1 shipped'
    Assert (-not (Get-Process -Name 'riptide-desktop' -ErrorAction SilentlyContinue)) 'the installer did not leave the app running'
    Assert-Registered $DirA 'fresh install'
    Test-HostPing $DirA 'fresh install'

    Step '2. upgrade over the same folder (stale manifest, missing key, user data)'
    Set-Content -LiteralPath (Join-Path $IntegrationDir 'integration.json') -Encoding UTF8 -Value '{"browser_host":true,"extra_extension_ids":["abcdefghijklmnopabcdefghijklmnop"]}'
    $manifestPath = Join-Path $IntegrationDir "native-messaging\$HostName.chromium.json"
    $stale = Read-Manifest $manifestPath
    $stale.allowed_origins = @('chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/')
    [IO.File]::WriteAllText($manifestPath, ($stale | ConvertTo-Json -Depth 4), (New-Object Text.UTF8Encoding($false)))
    Remove-Item -LiteralPath (RegPath ($RegRoot + '\' + $BrowserKeys['edge'])) -Recurse -Force
    Invoke-Installer @('/S', "/D=$DirA") 'upgrade'
    Assert-Registered $DirA 'upgrade'
    $c = Read-Manifest $manifestPath
    Assert (@($c.allowed_origins) -contains 'chrome-extension://abcdefghijklmnopabcdefghijklmnop/') 'upgrade : the user extra extension id survived'
    Assert (-not (@($c.allowed_origins) -contains 'chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/')) 'upgrade : the stale id was replaced'

    Step '3. a second copy in another folder takes the registration over'
    Invoke-Installer @('/S', "/D=$DirB") 'second install'
    Assert-Registered $DirB 'second install'

    Step '4. uninstalling copy A leaves copy B registered'
    Invoke-Uninstaller $DirA 'uninstall A'
    Assert (-not (Test-Path -LiteralPath (Join-Path $DirA 'riptide-host.exe'))) 'uninstall A : files removed'
    Assert-Registered $DirB 'after uninstall A'

    Step '5. uninstalling copy B removes the registration'
    Invoke-Uninstaller $DirB 'uninstall B'
    foreach ($b in $BrowserKeys.Keys) { Assert ($null -eq (Sandbox-Default $b)) "uninstall B : $b key removed" }
    Assert (-not (Test-Path -LiteralPath (Join-Path $IntegrationDir 'native-messaging'))) 'uninstall B : manifest folder removed'
    Assert (Test-Path -LiteralPath (Join-Path $IntegrationDir 'integration.json')) 'uninstall B : user settings kept (silent uninstall keeps data)'
}
catch {
    $script:Failed.Add("script error: $($_.Exception.Message)")
    Write-Host "ERROR: $($_.Exception.Message)" -ForegroundColor Red
}
finally {
    Step 'restoring the machine state'
    Restore-State
    foreach ($entry in $script:SavedKeys) {
        $after = Snapshot $entry.Key
        $same = ($entry.Before -eq $after)
        if ($entry.Key -like '*NativeMessagingHosts*') {
            Assert $same "real key unchanged: HKCU\$($entry.Key)"
        } else {
            Assert $same "restored exactly: HKCU\$($entry.Key)"
        }
    }
    if (-not $KeepTemp) { Remove-Item -LiteralPath $Temp -Recurse -Force -ErrorAction SilentlyContinue }
    Remove-Item Env:\RIPTIDE_REGISTRY_ROOT -ErrorAction SilentlyContinue
    Remove-Item Env:\RIPTIDE_INTEGRATION_DIR -ErrorAction SilentlyContinue
}

Write-Host ''
if ($script:Failed.Count -gt 0) {
    Write-Host "FAILED: $($script:Failed.Count) problem(s), $($script:Passed) assertion(s) passed" -ForegroundColor Red
    $script:Failed | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}
Write-Host "PASSED: $($script:Passed) assertions" -ForegroundColor Green
exit 0
