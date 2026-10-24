#Requires -Version 5.1
<#
.SYNOPSIS
    Real end-to-end self-update of Riptide against a local signed feed, on this machine,
    per user, no admin: an old build upgrades itself to a newer one, and a tampered package is
    refused.

.DESCRIPTION
    What it does:
      1. builds two disposable NSIS installers from the current commit (HEAD) in a throw-away git
         worktree, versions 0.1.0-beta.901 and 0.1.0-beta.902 by default, with the real updater
         key from %USERPROFILE%\.riptide-keys (read from the file, never printed, never copied
         into the repository). Both are built with the `update-e2e` cargo feature, which is what
         lets the app read RIPTIDE_UPDATE_ENDPOINT and run the update flow by itself at start
         (RIPTIDE_UPDATE_AUTOINSTALL=1). The release configuration is untouched: the e2e builds
         add `dangerousInsecureTransportProtocol` (http://127.0.0.1) through a --config overlay
         that exists only in this script.
      2. installs the OLD build silently into a temp folder (per user, /S /D=...), in portable
         mode (portable.txt next to the exe), so its database, logs and WebView2 profile live
         in <install>\data and never touch the real %LOCALAPPDATA%\Riptide.
      3. serves a signed manifest (latest-beta.json) and the installers from
         http://127.0.0.1:<port> (System.Net.HttpListener).
      4. TAMPERED RUN: the manifest names a package with one flipped byte next to the genuine
         signature. The app must find the update, refuse it with a signature error, keep running
         the old version and never start an installer.
      5. UPGRADE RUN: the manifest names the genuine package. The app downloads it, verifies the
         signature, stops the engine, runs the NSIS installer in passive mode, exits, and the
         installer starts the new version. The script asserts the new version is installed
         (exe version resource, Uninstall DisplayVersion, the new process runs from the install
         folder and logs its own version).
      6. restores the machine EXACTLY: the real HKCU\Software\riptide and Uninstall\Riptide keys,
         the Riptide shortcuts and the browser Native Messaging keys come back byte for byte
         (the same sandboxing as scripts/test-installer.ps1), a running installed Riptide is
         closed first (-StopInstalledApp) and started again at the end.

    Security reasoning for the test hook: the feed override is a compile-time cargo feature
    (`update-e2e`), not a runtime switch. A release binary does not contain the code that reads
    RIPTIDE_UPDATE_ENDPOINT, so no environment variable can redirect it. The feature is never
    enabled by the release pipeline (release.ps1 / release.yml do not pass --features). Even in
    an e2e build, the package still has to carry a valid minisign signature of the real key:
    the tampered run proves that.

    What a green run does NOT prove: nothing about GitHub itself (see verify-update-feed.ps1 for
    the published feed) and nothing about the click path in the Settings card (covered by the
    Angular specs): the e2e build runs the same Rust check/download/install code without a click.

.PARAMETER StopInstalledApp
    Close a running installed Riptide before the run and start it again afterwards. Without it
    the script refuses to run while riptide-desktop.exe is running (the installer would kill it).
.PARAMETER SkipBuild
    Reuse the installers of a previous run from -WorkDir\artifacts.
.PARAMETER OldVersion / NewVersion
    Versions of the two disposable builds. NewVersion must be a higher semver than OldVersion.
.PARAMETER WorkDir
    Where the worktree, the cargo target dir and the artifacts live between runs.
.PARAMETER KeyPath
    Updater private key (minisign). Its password comes from <key>.password next to it or from
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD.
.PARAMETER KeepTemp
    Keep the per-run temp folder (install folders, logs) for inspection.
.PARAMETER RemoveBuild
    Delete -WorkDir (worktree and target dir) at the end.
#>
[CmdletBinding()]
param(
    [switch]$StopInstalledApp,
    [switch]$SkipBuild,
    [string]$OldVersion = '0.1.0-beta.901',
    [string]$NewVersion = '0.1.0-beta.902',
    [string]$WorkDir = (Join-Path ([IO.Path]::GetTempPath()) 'rt-e2e'),
    [string]$KeyPath = (Join-Path $env:USERPROFILE '.riptide-keys\riptide-updater.key'),
    [switch]$KeepTemp,
    [switch]$RemoveBuild
)

$ErrorActionPreference = 'Stop'
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}
$RepoRoot = Split-Path -Parent $PSScriptRoot
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if ((Test-Path $cargoBin) -and ($env:PATH -notlike "*$cargoBin*")) { $env:PATH = "$cargoBin;$env:PATH" }

$script:Failed = New-Object System.Collections.Generic.List[string]
$script:Passed = 0
function Assert([bool]$Condition, [string]$Message) {
    if ($Condition) { $script:Passed++; Write-Host "  ok   $Message" -ForegroundColor Green }
    else { $script:Failed.Add($Message); Write-Host "  FAIL $Message" -ForegroundColor Red }
}
function Step([string]$Text) { Write-Host ''; Write-Host "==> $Text" -ForegroundColor Cyan }

# Native tools write progress to stderr; under 'Stop' Windows PowerShell 5.1 turns that into a
# terminating error even for success. Run them with 'Continue' and judge by the exit code.
function Invoke-Native([string]$What, [scriptblock]$Body) {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $global:LASTEXITCODE = 0
    try { & $Body 2>&1 | ForEach-Object { "$_" } | Where-Object { $_ -ne 'System.Management.Automation.RemoteException' } | Write-Host } finally { $ErrorActionPreference = $old }
    if ($LASTEXITCODE -ne 0) { throw "$What failed (exit $LASTEXITCODE)" }
}
function Invoke-Reg {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & reg.exe @args 2>&1 | Out-Null } finally { $ErrorActionPreference = $old }
}

# ------------------------------------------------------------------------------ guards
$installedApp = $null
$running = @(Get-Process -Name 'riptide-desktop' -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
    if (-not $StopInstalledApp) {
        Write-Host 'SKIPPED: Riptide (riptide-desktop.exe) is running. The installer would close it, and the' -ForegroundColor Yellow
        Write-Host '         single-instance lock stops a second copy. Close Riptide, or pass -StopInstalledApp' -ForegroundColor Yellow
        Write-Host '         to let this script close it and start it again at the end.' -ForegroundColor Yellow
        exit 0
    }
    $p = $running[0]
    $cim = Get-CimInstance Win32_Process -Filter "ProcessId=$($p.Id)" -ErrorAction SilentlyContinue
    $installedApp = @{ Path = $p.Path; CommandLine = $(if ($cim) { $cim.CommandLine } else { '' }) }
}
if ([version]($NewVersion -replace '-.*$', '') -lt [version]($OldVersion -replace '-.*$', '')) { throw 'NewVersion must be higher than OldVersion' }

# ------------------------------------------------------------------------------ sandbox
$Guid = [guid]::NewGuid().ToString('N').Substring(0, 8)
$Temp = Join-Path ([IO.Path]::GetTempPath()) "riptide-update-e2e-$Guid"
$Dir = Join-Path $Temp 'Riptide'
$IntegrationDir = Join-Path $Temp 'integration'
$Served = Join-Path $Temp 'served'
$Saved = Join-Path $Temp 'saved-state'
$RegRoot = "Software\RiptideUpdateE2E\$Guid"
New-Item -ItemType Directory -Force -Path $Temp, $Saved, $IntegrationDir, $Served | Out-Null

$HostName = 'app.riptide.host'
$BrowserKeys = @(
    "Software\Google\Chrome\NativeMessagingHosts\$HostName",
    "Software\Microsoft\Edge\NativeMessagingHosts\$HostName",
    "Software\Mozilla\NativeMessagingHosts\$HostName"
)
$GuardedKeys = @('Software\riptide', 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Riptide') + $BrowserKeys
$Shortcuts = @(
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'Riptide.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'Riptide\Riptide.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Riptide.lnk')
)
$script:SavedKeys = @()
$script:SavedLinks = @()
$script:Server = $null

function RegPath([string]$Key) { "HKCU:\$Key" }
function Snapshot([string]$Key) {
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("rt-snap-" + [guid]::NewGuid().ToString('N') + '.reg')
    Invoke-Reg export "HKCU\$Key" $tmp /y
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $tmp)) { return $null }
    $text = [IO.File]::ReadAllText($tmp)
    Remove-Item -LiteralPath $tmp -Force
    return $text
}
function Save-State {
    $i = 0
    foreach ($key in $GuardedKeys) {
        $i++
        $file = Join-Path $Saved "key$i.reg"
        $exists = Test-Path -LiteralPath (RegPath $key)
        $script:SavedKeys += , @{ Key = $key; File = $file; Existed = $exists; Before = (Snapshot $key) }
        if ($exists) { Invoke-Reg export "HKCU\$key" $file /y }
    }
    $j = 0
    foreach ($lnk in $Shortcuts) {
        $j++
        if (Test-Path -LiteralPath $lnk) {
            $copy = Join-Path $Saved "lnk$j.lnk"
            Copy-Item -LiteralPath $lnk -Destination $copy -Force
            $script:SavedLinks += , @{ Path = $lnk; Copy = $copy }
        }
    }
}
function Hide-RealInstall {
    # the installer finds a previous install through these keys and would run ITS uninstaller
    foreach ($key in 'Software\riptide', 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Riptide') {
        if (Test-Path -LiteralPath (RegPath $key)) { Remove-Item -LiteralPath (RegPath $key) -Recurse -Force }
    }
}
function Restore-State {
    foreach ($entry in $script:SavedKeys) {
        $p = RegPath $entry.Key
        if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Recurse -Force -ErrorAction SilentlyContinue }
        if ($entry.Existed) { try { Invoke-Reg import $entry.File } catch { Write-Host "  restore failed for $($entry.Key): $_" -ForegroundColor Red } }
    }
    foreach ($lnk in $Shortcuts) {
        if (Test-Path -LiteralPath $lnk) { Remove-Item -LiteralPath $lnk -Force -ErrorAction SilentlyContinue }
    }
    foreach ($l in $script:SavedLinks) {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $l.Path) | Out-Null
        Copy-Item -LiteralPath $l.Copy -Destination $l.Path -Force
    }
    $root = RegPath $RegRoot
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue }
    $parent = RegPath 'Software\RiptideUpdateE2E'
    if ((Test-Path -LiteralPath $parent) -and -not (Get-ChildItem -LiteralPath $parent)) { Remove-Item -LiteralPath $parent -Force }
}

# ------------------------------------------------------------------------------ build
function Get-Key {
    if (-not (Test-Path -LiteralPath $KeyPath)) { throw "Updater private key not found at $KeyPath" }
    $env:TAURI_SIGNING_PRIVATE_KEY = ([IO.File]::ReadAllText($KeyPath)).Trim()
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
        $pw = [IO.Path]::ChangeExtension($KeyPath, '.password')
        if (-not (Test-Path -LiteralPath $pw)) { throw "Signing key password not found: set TAURI_SIGNING_PRIVATE_KEY_PASSWORD or create $pw" }
        $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ([IO.File]::ReadAllText($pw)).Trim()
    }
}

function Build-Version([string]$Version, [string]$Src, [string]$Target, [string]$Out) {
    $cargoToml = Join-Path $Src 'Cargo.toml'
    $text = [IO.File]::ReadAllText($cargoToml)
    $re = New-Object Text.RegularExpressions.Regex '(?m)^(version\s*=\s*)"[^"]+"'
    [IO.File]::WriteAllText($cargoToml, $re.Replace($text, ('${1}"' + $Version + '"'), 1), (New-Object Text.UTF8Encoding($false)))
    $conf = Join-Path $WorkDir 'tauri.e2e.conf.json'
    $json = @{
        build   = @{ beforeBuildCommand = 'npm run build' }
        bundle  = @{ createUpdaterArtifacts = $true; externalBin = @('binaries/riptide-host') }
        plugins = @{ updater = @{ dangerousInsecureTransportProtocol = $true; endpoints = @('http://127.0.0.1:9/latest.json') } }
    } | ConvertTo-Json -Depth 6
    [IO.File]::WriteAllText($conf, $json, (New-Object Text.UTF8Encoding($false)))
    $env:CARGO_TARGET_DIR = $Target
    Push-Location (Join-Path $Src 'apps\desktop')
    try {
        Write-Host "  building $Version (release, update-e2e feature) ..."
        Invoke-Native "tauri build $Version" { npx tauri build --ci --config $conf --features update-e2e }
    } finally { Pop-Location; Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue }
    $nsis = Join-Path $Target 'release\bundle\nsis'
    $exe = Get-ChildItem $nsis -Filter "Riptide_${Version}_*-setup.exe" | Select-Object -First 1
    if ($null -eq $exe -or -not (Test-Path -LiteralPath "$($exe.FullName).sig")) { throw "no signed installer for $Version in $nsis" }
    $dest = Join-Path $Out $Version
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Copy-Item $exe.FullName, "$($exe.FullName).sig" -Destination $dest -Force
}

function Initialize-Worktree([string]$Src) {
    if (-not (Test-Path -LiteralPath (Join-Path $Src 'Cargo.toml'))) {
        Invoke-Native 'git worktree add' { git -C $RepoRoot worktree add --detach $Src HEAD }
    } else {
        # a worktree left by an earlier run: move it to the current commit (drops its version edit)
        $head = (& git -C $RepoRoot rev-parse HEAD).Trim()
        Invoke-Native 'git checkout' { git -C $Src checkout -f --detach $head }
    }
    $nm = Join-Path $Src 'apps\desktop\node_modules'
    if (-not (Test-Path -LiteralPath $nm)) {
        Invoke-Native 'node_modules junction' { cmd.exe /c mklink /J "$nm" "$(Join-Path $RepoRoot 'apps\desktop\node_modules')" }
    }
    $bin = Join-Path $Src 'apps\desktop\src-tauri\binaries'
    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    Get-ChildItem (Join-Path $RepoRoot 'apps\desktop\src-tauri\binaries') -File | ForEach-Object {
        if (-not (Test-Path -LiteralPath (Join-Path $bin $_.Name))) { Copy-Item $_.FullName $bin }
    }
}

# ------------------------------------------------------------------------------ feed server
function Get-FreePort {
    $l = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 0)
    $l.Start(); $port = $l.LocalEndpoint.Port; $l.Stop(); return $port
}
function Start-FeedServer([string]$Root, [int]$Port) {
    $listener = New-Object Net.HttpListener
    $listener.Prefixes.Add("http://127.0.0.1:$Port/")
    $listener.Start()
    $ps = [powershell]::Create()
    [void]$ps.AddScript({
        param($listener, $root)
        while ($listener.IsListening) {
            try { $ctx = $listener.GetContext() } catch { break }
            try {
                $name = [Uri]::UnescapeDataString($ctx.Request.Url.AbsolutePath.TrimStart('/'))
                $file = Join-Path $root $name
                if ($name -and $name -notmatch '[\\/]' -and (Test-Path -LiteralPath $file -PathType Leaf)) {
                    $bytes = [IO.File]::ReadAllBytes($file)
                    $ctx.Response.StatusCode = 200
                    $ctx.Response.ContentType = if ($name -like '*.json') { 'application/json' } else { 'application/octet-stream' }
                    $ctx.Response.ContentLength64 = $bytes.Length
                    $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
                } else { $ctx.Response.StatusCode = 404 }
            } catch { } finally { try { $ctx.Response.Close() } catch { } }
        }
    }).AddArgument($listener).AddArgument($Root)
    $handle = $ps.BeginInvoke()
    return @{ Listener = $listener; Shell = $ps; Handle = $handle }
}
function Stop-FeedServer {
    if ($null -eq $script:Server) { return }
    try { $script:Server.Listener.Stop(); $script:Server.Listener.Close() } catch { }
    try { $script:Server.Shell.Stop(); $script:Server.Shell.Dispose() } catch { }
    $script:Server = $null
}

function Write-Manifest([string]$Version, [string]$PackageName, [string]$Signature, [int]$Port) {
    $manifest = [ordered]@{
        version   = $Version
        notes     = "e2e build $Version"
        pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss'Z'")
        platforms = [ordered]@{
            'windows-x86_64' = [ordered]@{ signature = $Signature; url = "http://127.0.0.1:$Port/$PackageName" }
        }
    }
    [IO.File]::WriteAllText((Join-Path $Served 'latest-beta.json'), ($manifest | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding($false)))
}

# ------------------------------------------------------------------------------ app helpers
function Read-SharedText([string]$Path) {
    $fs = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite')
    try { $sr = New-Object IO.StreamReader($fs); return $sr.ReadToEnd() } finally { $fs.Dispose() }
}
function Get-AppLog {
    $logs = Join-Path $Dir 'data\logs'
    if (-not (Test-Path -LiteralPath $logs)) { return '' }
    $text = ''
    Get-ChildItem -LiteralPath $logs -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | ForEach-Object { $text += (Read-SharedText $_.FullName) }
    return $text
}
function Wait-For([scriptblock]$Condition, [int]$Seconds, [string]$What) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if (& $Condition) { return $true }
        Start-Sleep -Milliseconds 500
    }
    Write-Host "  (timed out after ${Seconds}s waiting for: $What)" -ForegroundColor Yellow
    return $false
}
function Get-InstalledApps { @(Get-Process -Name 'riptide-desktop' -ErrorAction SilentlyContinue | Where-Object { $_.Path -ieq (Join-Path $Dir 'riptide-desktop.exe') }) }
function Stop-SandboxApp {
    foreach ($p in (Get-InstalledApps)) { try { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } catch { } }
    [void](Wait-For { (Get-InstalledApps).Count -eq 0 } 15 'the app to exit')
}
function Start-SandboxApp([int]$Port) {
    # the feed itself comes from update-e2e-endpoint.txt next to the exe (written below), because the
    # app the installer relaunches does not inherit this environment
    $env:RIPTIDE_UPDATE_AUTOINSTALL = '1'
    try { return Start-Process -FilePath (Join-Path $Dir 'riptide-desktop.exe') -PassThru }
    finally {
        # the process has its copy; later children (the installer) inherit it from the app
        Remove-Item Env:\RIPTIDE_UPDATE_AUTOINSTALL -ErrorAction SilentlyContinue
    }
}
function Get-ExeVersion { (Get-Item -LiteralPath (Join-Path $Dir 'riptide-desktop.exe')).VersionInfo.ProductVersion }
function Get-UninstallVersion {
    $k = RegPath 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Riptide'
    if (Test-Path -LiteralPath $k) { return (Get-ItemProperty -LiteralPath $k).DisplayVersion }
    return $null
}

# ------------------------------------------------------------------------------ run
Write-Host "Old / new  : $OldVersion -> $NewVersion"
Write-Host "Work dir   : $WorkDir"
Write-Host "Temp       : $Temp"
Write-Host "Install in : $Dir"

try {
    $src = Join-Path $WorkDir 'src'
    $target = Join-Path $WorkDir 'target'
    $artifacts = Join-Path $WorkDir 'artifacts'
    New-Item -ItemType Directory -Force -Path $WorkDir, $artifacts | Out-Null

    Step '0. disposable signed installers'
    $haveBoth = (Test-Path (Join-Path $artifacts "$OldVersion\*-setup.exe.sig")) -and (Test-Path (Join-Path $artifacts "$NewVersion\*-setup.exe.sig"))
    if ($SkipBuild -and -not $haveBoth) { throw "-SkipBuild, but $artifacts has no installers for both versions" }
    if (-not ($SkipBuild -and $haveBoth)) {
        Get-Key
        try {
            Initialize-Worktree $src
            Build-Version $OldVersion $src $target $artifacts
            Build-Version $NewVersion $src $target $artifacts
        } finally {
            Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
            Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
        }
    }
    $oldInstaller = Get-ChildItem (Join-Path $artifacts $OldVersion) -Filter '*-setup.exe' | Select-Object -First 1
    $newInstaller = Get-ChildItem (Join-Path $artifacts $NewVersion) -Filter '*-setup.exe' | Select-Object -First 1
    Assert ($null -ne $oldInstaller -and $null -ne $newInstaller) 'both installers and their .sig files exist'
    $newSig = ([IO.File]::ReadAllText("$($newInstaller.FullName).sig")).Trim()
    Assert ($newSig.Length -gt 100) 'the new installer has a minisign signature (value not printed)'

    Step '1. sandbox the machine state and install the OLD build'
    Save-State
    if ($installedApp) {
        Write-Host "  closing the installed Riptide ($($installedApp.Path)) ..."
        foreach ($p in $running) { [void]$p.CloseMainWindow() }
        [void](Wait-For { @(Get-Process -Name 'riptide-desktop' -ErrorAction SilentlyContinue).Count -eq 0 } 8 'the installed app to close')
        foreach ($p in @(Get-Process -Name 'riptide-desktop' -ErrorAction SilentlyContinue)) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
        [void](Wait-For { @(Get-Process -Name 'riptide-desktop' -ErrorAction SilentlyContinue).Count -eq 0 } 15 'the installed app to exit')
    }
    Hide-RealInstall
    $env:RIPTIDE_REGISTRY_ROOT = $RegRoot
    $env:RIPTIDE_INTEGRATION_DIR = $IntegrationDir
    $p = Start-Process -FilePath $oldInstaller.FullName -ArgumentList @('/S', "/D=$Dir") -Wait -PassThru
    Assert ($p.ExitCode -eq 0) "old build installed silently (exit code $($p.ExitCode))"
    Assert (Test-Path -LiteralPath (Join-Path $Dir 'riptide-desktop.exe')) 'riptide-desktop.exe installed in the temp folder'
    # A silent install (/S) skips the language dialog and so never stores the language; a normal
    # interactive install stores "Installer Language" under HKCU\Software\riptide\Riptide. Without
    # it the passive update installer stops at the "Installer Language" dialog and waits for a
    # click. Seed what an interactive install writes, so this run mirrors a real user.
    Set-ItemProperty -LiteralPath (RegPath 'Software\riptide\Riptide') -Name 'Installer Language' -Value '1033'
    [IO.File]::WriteAllText((Join-Path $Dir 'portable.txt'), 'update e2e: keep data next to the exe', (New-Object Text.UTF8Encoding($false)))
    Assert ((Get-ExeVersion) -eq $OldVersion) "installed exe reports $OldVersion (is: $(Get-ExeVersion))"
    Assert ((Get-UninstallVersion) -eq $OldVersion) "Uninstall DisplayVersion is $OldVersion"
    $oldHash = (Get-FileHash -LiteralPath (Join-Path $Dir 'riptide-desktop.exe') -Algorithm SHA256).Hash

    Step '2. local signed feed on 127.0.0.1'
    $port = Get-FreePort
    Copy-Item $newInstaller.FullName (Join-Path $Served $newInstaller.Name)
    $tampered = [IO.File]::ReadAllBytes($newInstaller.FullName)
    $tampered[[int]($tampered.Length / 2)] = $tampered[[int]($tampered.Length / 2)] -bxor 0x01
    [IO.File]::WriteAllBytes((Join-Path $Served 'tampered-setup.exe'), $tampered)
    $script:Server = Start-FeedServer $Served $port
    [IO.File]::WriteAllText((Join-Path $Dir 'update-e2e-endpoint.txt'), "http://127.0.0.1:$port/latest-beta.json", (New-Object Text.UTF8Encoding($false)))
    Write-Manifest $NewVersion $newInstaller.Name $newSig $port
    $probe = (New-Object Net.WebClient).DownloadString("http://127.0.0.1:$port/latest-beta.json")
    Assert ($probe -match [regex]::Escape($NewVersion)) "feed answers with $NewVersion"

    Step '3. TAMPERED package: one flipped byte, genuine signature'
    Write-Manifest $NewVersion 'tampered-setup.exe' $newSig $port
    $app = Start-SandboxApp $port
    $sawRefusal = Wait-For { (Get-AppLog) -match 'e2e update: install refused' } 90 'the app to refuse the tampered package'
    $log = Get-AppLog
    Assert ($log -match "e2e update: running version.*$([regex]::Escape($OldVersion))") "the app ran as $OldVersion"
    Assert ($log -match 'e2e update: update found') 'the app found the update in the feed'
    Assert $sawRefusal 'the app refused to install'
    Assert ($log -match 'e2e update: install refused[^\r\n]*Signature') 'the refusal says Signature'
    Start-Sleep -Seconds 3
    Assert (-not $app.HasExited) 'the old app is still running (nothing was installed over it)'
    Assert ((Get-ExeVersion) -eq $OldVersion) "installed exe is still $OldVersion"
    Assert (((Get-FileHash -LiteralPath (Join-Path $Dir 'riptide-desktop.exe') -Algorithm SHA256).Hash) -eq $oldHash) 'installed exe is byte-identical to before'
    Assert ((Get-UninstallVersion) -eq $OldVersion) "Uninstall DisplayVersion is still $OldVersion"
    Stop-SandboxApp
    Remove-Item -LiteralPath (Join-Path $Dir 'data\logs') -Recurse -Force -ErrorAction SilentlyContinue

    Step '4. UPGRADE: genuine package, genuine signature'
    Write-Manifest $NewVersion $newInstaller.Name $newSig $port
    $app = Start-SandboxApp $port
    $found = Wait-For { (Get-AppLog) -match 'e2e update: update found' } 60 'the update to be found'
    Assert $found 'the old app found the update'
    # the installer replaces the files; the new version starts by itself and logs its version
    $upgraded = Wait-For { (Get-ExeVersion) -eq $NewVersion } 180 'the installer to replace the app'
    Assert $upgraded "installed exe now reports $NewVersion (is: $(Get-ExeVersion))"
    $verRe = [regex]::Escape($NewVersion)
    $relaunched = Wait-For { (Get-InstalledApps).Count -gt 0 -and (Get-AppLog) -match "data directory resolved version=`"?$verRe" } 90 'the new version to start'
    Assert $relaunched "the installer started the new version ($NewVersion) from the install folder, and it logs that version"
    $log = Get-AppLog
    if (-not $relaunched) {
        Write-Host '  diagnostics (the new version did not start by itself):' -ForegroundColor Yellow
        Get-CimInstance Win32_Process -Filter "Name like 'riptide%' or Name like 'Riptide%'" | ForEach-Object { Write-Host "    process $($_.ProcessId) $($_.ExecutablePath) :: $($_.CommandLine)" }
        ($log -split "`n" | Where-Object { $_ -match 'e2e update|update|panic|error|exit' } | Select-Object -Last 15) | ForEach-Object { Write-Host "    log: $_" }
    }
    Assert ($log -notmatch 'e2e update: install refused') 'no refusal during the upgrade'
    $sawUpToDate = Wait-For { (Get-AppLog).Substring([Math]::Max(0, (Get-AppLog).LastIndexOf('installing update'))) -match 'update check[^\r\n]*available=None' } 90 'the new version to check the local feed'
    Assert $sawUpToDate "the new version (started by the installer) checks the feed and finds nothing newer than $NewVersion"
    Assert ((Get-UninstallVersion) -eq $NewVersion) "Uninstall DisplayVersion is $NewVersion"
    Assert (Test-Path -LiteralPath (Join-Path $Dir 'portable.txt')) 'portable.txt (and with it the data folder) survived the update'
    Assert (-not (Get-Process -Name 'Riptide_*setup*' -ErrorAction SilentlyContinue)) 'the installer is gone'
    Stop-SandboxApp

    Step '5. uninstall the sandbox copy'
    $un = Join-Path $Dir 'uninstall.exe'
    if (Test-Path -LiteralPath $un) {
        $p = Start-Process -FilePath $un -ArgumentList @('/S', "_?=$Dir") -Wait -PassThru
        Assert ($p.ExitCode -eq 0) "uninstaller exit code $($p.ExitCode)"
        Remove-Item -LiteralPath $un -Force -ErrorAction SilentlyContinue
    }
}
catch {
    $script:Failed.Add("script error: $($_.Exception.Message)")
    Write-Host "ERROR: $($_.Exception.Message)" -ForegroundColor Red
}
finally {
    Step 'restoring the machine state'
    Stop-FeedServer
    Remove-Item Env:\RIPTIDE_REGISTRY_ROOT, Env:\RIPTIDE_INTEGRATION_DIR, Env:\RIPTIDE_UPDATE_AUTOINSTALL -ErrorAction SilentlyContinue
    foreach ($p in @(Get-Process -Name 'riptide-desktop' -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($Temp, [StringComparison]::OrdinalIgnoreCase) })) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Seconds 1
    Restore-State
    foreach ($entry in $script:SavedKeys) {
        $after = Snapshot $entry.Key
        Assert ($entry.Before -eq $after) "restored exactly: HKCU\$($entry.Key)"
    }
    if ($installedApp -and (Test-Path -LiteralPath $installedApp.Path)) {
        Write-Host "  starting the installed Riptide again ($($installedApp.Path))"
        Start-Process -FilePath $installedApp.Path
        $back = Wait-For { @(Get-Process -Name 'riptide-desktop' -ErrorAction SilentlyContinue).Count -gt 0 } 20 'the installed app to start'
        Assert $back 'the installed Riptide runs again'
    }
    if (-not $KeepTemp) { Remove-Item -LiteralPath $Temp -Recurse -Force -ErrorAction SilentlyContinue }
    if ($RemoveBuild) {
        Invoke-Native 'git worktree remove' { git -C $RepoRoot worktree remove --force (Join-Path $WorkDir 'src') }
        Remove-Item -LiteralPath $WorkDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ''
if ($script:Failed.Count -gt 0) {
    Write-Host "FAILED: $($script:Failed.Count) problem(s), $($script:Passed) assertion(s) passed" -ForegroundColor Red
    $script:Failed | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}
Write-Host "PASSED: $($script:Passed) assertions" -ForegroundColor Green
exit 0
