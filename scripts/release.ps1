#Requires -Version 5.1
<#
.SYNOPSIS
    Builds a signed Riptide release locally and prints the commands that publish it.

.DESCRIPTION
    1. Bumps the version: [workspace.package] in Cargo.toml (the desktop app, its bundle and
       `app_info` all read it), Cargo.lock, and apps/desktop/package.json + package-lock.json.
       The browser extension is versioned on its own (store versions are numeric only).
    2. Runs `npm run bundle` (NSIS per-user installer, plus the updater `.sig` through
       src-tauri/tauri.release.conf.json) and `npm run bundle:portable`, with
       TAURI_SIGNING_PRIVATE_KEY read from the key file (never from the repository).
    3. Collects installer, `.sig` and portable zip in target/release-artifacts/<version>/,
       writes latest-beta.json (always) and latest.json (stable releases only) with the
       `windows-x86_64` platform entry, and SHA256SUMS.txt.
    4. Prints the `gh` commands. It never publishes, commits, tags or pushes anything.

    Update channels (see apps/desktop/src-tauri/src/updater.rs):
      stable  https://github.com/LayanJarrar1/Riptide/releases/latest/download/latest.json
              `latest` is the newest release NOT marked pre-release, so latest.json is only
              attached to stable releases.
      beta    https://github.com/LayanJarrar1/Riptide/releases/download/channel-beta/latest-beta.json
              GitHub has no "latest including pre-releases" link, so every release (stable or
              not) re-uploads latest-beta.json to the fixed pre-release `channel-beta`.

.PARAMETER Version
    Semver without the leading v, e.g. 0.1.0-alpha.3.
.PARAMETER Prerelease
    Mark it a pre-release: no latest.json, only the beta channel sees it.
.PARAMETER KeyPath
    Updater private key (minisign, from `npx tauri signer generate`). Default
    %USERPROFILE%\.riptide-keys\riptide-updater.key. Its password, if any, comes from
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD, or a <key>.password file next to the key.
.PARAMETER Notes
    Release notes for the update manifests (shown in Settings > About).
.PARAMETER SkipBump
    Build the version already in the tree.

.EXAMPLE
    .\scripts\release.ps1 -Version 0.1.0-alpha.3 -Prerelease -Notes "Self-update, clean uninstall"
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [switch]$Prerelease,
    [string]$KeyPath = (Join-Path $env:USERPROFILE '.riptide-keys\riptide-updater.key'),
    [string]$Notes = '',
    [switch]$SkipBump
)

$ErrorActionPreference = 'Stop'
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$Repo = 'LayanJarrar1/Riptide'
$RepoRoot = Split-Path -Parent $PSScriptRoot
$Desktop = Join-Path $RepoRoot 'apps\desktop'
$Utf8 = New-Object System.Text.UTF8Encoding $false

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if ((Test-Path $cargoBin) -and ($env:PATH -notlike "*$cargoBin*")) { $env:PATH = "$cargoBin;$env:PATH" }

function Step([string]$Text) { Write-Host ''; Write-Host "==> $Text" -ForegroundColor Cyan }

# Runs a native command; stderr progress is not an error, the exit code is.
function Invoke-Native([string]$What, [scriptblock]$Body) {
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Body; $code = $LASTEXITCODE } finally { $ErrorActionPreference = $saved }
    if ($code -ne 0) { throw "$What failed (exit $code)" }
}

if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
    throw "Version '$Version' is not semver (expected e.g. 0.1.0 or 0.1.0-alpha.3, no leading v)"
}
if (-not (Test-Path $KeyPath)) {
    throw "Updater private key not found at $KeyPath (generate it with: npx tauri signer generate -w `"$KeyPath`")"
}
$Tag = "v$Version"
if ($Notes -eq '') { $Notes = "Riptide $Version" }

# ---------------------------------------------------------------- 1. version bump
if (-not $SkipBump) {
    Step "Version -> $Version"
    $cargoToml = Join-Path $RepoRoot 'Cargo.toml'
    $text = [IO.File]::ReadAllText($cargoToml)
    $pattern = '(?ms)(^\[workspace\.package\]\s*$.*?^version\s*=\s*")[^"]*(")'
    if ($text -notmatch $pattern) { throw 'no version under [workspace.package] in Cargo.toml' }
    $text = [regex]::Replace($text, $pattern, "`${1}$Version`${2}", 1)
    [IO.File]::WriteAllText($cargoToml, $text, $Utf8)

    Push-Location $Desktop
    try {
        Invoke-Native 'npm version' { npm version $Version --no-git-tag-version --allow-same-version | Out-Null }
    } finally { Pop-Location }

    Push-Location $RepoRoot
    try {
        # only the workspace members' own entries change; offline first, the index if needed
        $saved = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        cargo update --workspace --offline 2>&1 | Out-Null
        $offline = $LASTEXITCODE
        $ErrorActionPreference = $saved
        if ($offline -ne 0) {
            Invoke-Native 'cargo update --workspace' { cargo update --workspace 2>&1 | Out-Null }
        }
    } finally { Pop-Location }
}

# ---------------------------------------------------------------- 2. build
$env:TAURI_SIGNING_PRIVATE_KEY = ([IO.File]::ReadAllText($KeyPath)).Trim()
if (-not $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
    # PowerShell cannot hold an empty environment variable, so the key always has a password,
    # kept in a file next to it (outside the repository).
    $passwordFile = [IO.Path]::ChangeExtension($KeyPath, '.password')
    if (-not (Test-Path $passwordFile)) { throw "Signing key password not found: set TAURI_SIGNING_PRIVATE_KEY_PASSWORD or create $passwordFile" }
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ([IO.File]::ReadAllText($passwordFile)).Trim()
}

Push-Location $Desktop
try {
    Step 'npm run bundle (installer + updater signature)'
    Invoke-Native 'npm run bundle' { npm run bundle -- --config src-tauri/tauri.release.conf.json }
    Step 'npm run bundle:portable'
    Invoke-Native 'npm run bundle:portable' { npm run bundle:portable }
} finally {
    Pop-Location
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
}

# ---------------------------------------------------------------- 3. collect
Step 'Collecting artifacts'
$nsisDir = Join-Path $RepoRoot 'target\release\bundle\nsis'
$installer = Get-ChildItem $nsisDir -Filter "Riptide_${Version}_*-setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($null -eq $installer) { throw "no installer for $Version in $nsisDir" }
$sigFile = "$($installer.FullName).sig"
if (-not (Test-Path $sigFile)) { throw "no updater signature next to $($installer.Name)" }
$portable = Join-Path $RepoRoot "target\portable\Riptide-$Version-portable.zip"
if (-not (Test-Path $portable)) { throw "no portable zip at $portable" }

$out = Join-Path $RepoRoot "target\release-artifacts\$Version"
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item $installer.FullName, $sigFile, $portable -Destination $out

$manifest = [ordered]@{
    version   = $Version
    notes     = $Notes
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss'Z'")
    platforms = [ordered]@{
        'windows-x86_64' = [ordered]@{
            signature = ([IO.File]::ReadAllText($sigFile)).Trim()
            url       = "https://github.com/$Repo/releases/download/$Tag/$($installer.Name)"
        }
    }
}
$json = $manifest | ConvertTo-Json -Depth 5
[IO.File]::WriteAllText((Join-Path $out 'latest-beta.json'), $json, $Utf8)
if (-not $Prerelease) {
    [IO.File]::WriteAllText((Join-Path $out 'latest.json'), $json, $Utf8)
}

$sums = Get-ChildItem $out -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | Sort-Object Name | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $_.Name
}
[IO.File]::WriteAllText((Join-Path $out 'SHA256SUMS.txt'), (($sums -join "`n") + "`n"), $Utf8)

Write-Host ''
Get-ChildItem $out -File | ForEach-Object { Write-Host ("  {0,-48} {1,12:n0} bytes" -f $_.Name, $_.Length) }
Write-Host ''
$sums | ForEach-Object { Write-Host "  $_" }

# ---------------------------------------------------------------- 4. publish commands (printed only)
$assets = (Get-ChildItem $out -File | Sort-Object Name | ForEach-Object { "`"$($_.FullName)`"" }) -join ' '
$pre = if ($Prerelease) { ' --prerelease' } else { ' --latest' }
Write-Host ''
Write-Host 'Nothing was published. Review the artifacts and commit the version bump.' -ForegroundColor Yellow
Write-Host 'With GitHub Actions available, pushing the tag is enough (.github/workflows/release.yml builds'
Write-Host 'and publishes the same files). Otherwise publish these local artifacts by hand:'
Write-Host ''
Write-Host "  git tag $Tag; git push origin $Tag"
Write-Host "  gh release create $Tag $assets --repo $Repo --title `"Riptide $Version`" --notes `"$Notes`"$pre"
Write-Host "  gh release view channel-beta --repo $Repo *> `$null; if (-not `$?) { gh release create channel-beta --repo $Repo --prerelease --title `"Beta update channel`" --notes `"Holds latest-beta.json for the beta update channel. Not a release.`" }"
Write-Host "  gh release upload channel-beta `"$(Join-Path $out 'latest-beta.json')`" --repo $Repo --clobber"
Write-Host ''
