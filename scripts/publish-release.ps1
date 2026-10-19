#Requires -Version 5.1
<#
.SYNOPSIS
    Publishes the artifacts built by scripts/release.ps1 and checks that the update feed works.

.DESCRIPTION
    Without -Publish this is a DRY RUN: it runs every safety check, prints what it would do and
    changes nothing. With -Publish it
      1. creates the GitHub release v<Version> (a pre-release when the version has a '-') with the
         installer, its .sig, the portable zip, SHA256SUMS.txt, latest-beta.json (and latest.json
         for stable versions);
      2. makes sure the fixed pre-release `channel-beta` exists and uploads latest-beta.json to it
         with --clobber (this is the file the beta channel of the app reads);
      3. runs scripts/verify-update-feed.ps1 -ExpectVersion <Version>.

    It refuses to publish when the git tree is dirty, when HEAD is not contained in origin/main,
    or when the artifacts do not match their SHA256SUMS.txt. The tag is created by GitHub on the
    commit you are on; nothing is tagged or pushed locally.

.PARAMETER Version
    Semver without the leading v, e.g. 0.1.0-alpha.5.
.PARAMETER ArtifactsDir
    Default target\release-artifacts\<Version> (where release.ps1 puts them).
.PARAMETER NotesFile
    Markdown release notes. Without it GitHub generates the notes.
.PARAMETER Publish
    Actually publish. Without it, dry run.
.PARAMETER Repo
    owner/name. Default LayanJarrar1/Riptide.

.EXAMPLE
    .\scripts\publish-release.ps1 -Version 0.1.0-alpha.5 -NotesFile notes.md          # dry run
    .\scripts\publish-release.ps1 -Version 0.1.0-alpha.5 -NotesFile notes.md -Publish
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$ArtifactsDir = '',
    [string]$NotesFile = '',
    [switch]$Publish,
    [string]$Repo = 'LayanJarrar1/Riptide'
)

$ErrorActionPreference = 'Stop'
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Tag = "v$Version"
$Stable = $Version -notmatch '-'
$DryRun = -not $Publish.IsPresent
$script:refusals = New-Object System.Collections.Generic.List[string]

function Step([string]$Text) { Write-Host ''; Write-Host "==> $Text" -ForegroundColor Cyan }
function Refuse([string]$Text) {
    $script:refusals.Add($Text)
    if ($DryRun) { Write-Host "  WOULD REFUSE  $Text" -ForegroundColor Yellow }
    else { throw $Text }
}

# Runs a native command; returns its exit code and output. stderr text is not an error here.
function Invoke-Native([scriptblock]$Body) {
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $Body 2>&1; $code = $LASTEXITCODE } finally { $ErrorActionPreference = $saved }
    return [pscustomobject]@{ Code = $code; Output = ($out | ForEach-Object { "$_" }) }
}

if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
    throw "Version '$Version' is not semver (expected e.g. 0.1.0-alpha.5, no leading v)"
}
if (-not $ArtifactsDir) { $ArtifactsDir = Join-Path $RepoRoot "target\release-artifacts\$Version" }

$gh = Get-Command gh -ErrorAction SilentlyContinue
$ghPath = if ($gh) { $gh.Source } else { 'C:\Program Files\GitHub CLI\gh.exe' }
if (-not (Test-Path $ghPath)) { throw 'GitHub CLI (gh) not found. Install it and run: gh auth login' }

if ($DryRun) { Write-Host 'DRY RUN: nothing will be changed. Add -Publish to publish.' -ForegroundColor Yellow }

# ---------------------------------------------------------------- 1. git state
Step 'Git state'
Push-Location $RepoRoot
try {
    $dirty = Invoke-Native { git status --porcelain }
    if ($dirty.Output.Count -gt 0 -and "$($dirty.Output -join '')".Trim()) { Refuse 'the git tree is dirty (commit or stash first)' }
    else { Write-Host '  tree is clean' }

    $fetch = Invoke-Native { git fetch origin main --quiet }
    if ($fetch.Code -ne 0) { Refuse 'cannot fetch origin/main' }
    $head = (Invoke-Native { git rev-parse HEAD }).Output | Select-Object -First 1
    $onMain = Invoke-Native { git merge-base --is-ancestor HEAD origin/main }
    if ($onMain.Code -ne 0) { Refuse 'HEAD is not on origin/main (merge the release PR and check out main first)' }
    else { Write-Host "  HEAD $head is on origin/main" }
} finally { Pop-Location }

# ---------------------------------------------------------------- 2. artifacts
Step "Artifacts in $ArtifactsDir"
if (-not (Test-Path $ArtifactsDir)) { throw "artifacts folder not found: $ArtifactsDir (run scripts\release.ps1 first)" }
$installer = Get-ChildItem $ArtifactsDir -Filter "Riptide_${Version}_*-setup.exe" | Select-Object -First 1
$required = @(
    $(if ($installer) { $installer.Name } else { "Riptide_${Version}_x64-setup.exe" }),
    $(if ($installer) { "$($installer.Name).sig" } else { "Riptide_${Version}_x64-setup.exe.sig" }),
    "Riptide-$Version-portable.zip",
    'SHA256SUMS.txt',
    'latest-beta.json'
)
if ($Stable) { $required += 'latest.json' }
foreach ($name in $required) {
    if (-not (Test-Path (Join-Path $ArtifactsDir $name))) { Refuse "missing artifact: $name" }
}
if (-not $Stable -and (Test-Path (Join-Path $ArtifactsDir 'latest.json'))) {
    Refuse 'latest.json present for a pre-release (it would feed the stable channel)'
}

$sumsPath = Join-Path $ArtifactsDir 'SHA256SUMS.txt'
if (Test-Path $sumsPath) {
    $listed = @{}
    foreach ($line in [IO.File]::ReadAllLines($sumsPath)) {
        if (-not $line.Trim()) { continue }
        if ($line -notmatch '^([0-9a-fA-F]{64})\s+(.+)$') { Refuse "malformed line in SHA256SUMS.txt: $line"; continue }
        $listed[$Matches[2].Trim()] = $Matches[1].ToLowerInvariant()
    }
    foreach ($name in $listed.Keys) {
        $file = Join-Path $ArtifactsDir $name
        if (-not (Test-Path $file)) { Refuse "SHA256SUMS.txt lists $name but the file is missing"; continue }
        $actual = (Get-FileHash $file -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $listed[$name]) { Refuse "SHA-256 mismatch for $name" } else { Write-Host "  sha256 ok  $name" }
    }
    foreach ($f in Get-ChildItem $ArtifactsDir -File) {
        if ($f.Name -ne 'SHA256SUMS.txt' -and -not $listed.ContainsKey($f.Name)) { Refuse "$($f.Name) is not covered by SHA256SUMS.txt" }
    }
}

$betaPath = Join-Path $ArtifactsDir 'latest-beta.json'
if (Test-Path $betaPath) {
    $feed = [IO.File]::ReadAllText($betaPath).TrimStart([char]0xFEFF) | ConvertFrom-Json
    if ($feed.version -ne $Version) { Refuse "latest-beta.json says version '$($feed.version)', not $Version" }
    $win = $feed.platforms.'windows-x86_64'
    if (-not $win -or -not $win.signature -or -not $win.url) { Refuse 'latest-beta.json has no windows-x86_64 url and signature' }
    elseif ($win.url -notlike "https://github.com/$Repo/releases/download/$Tag/*") { Refuse "latest-beta.json url does not point at the $Tag release of $Repo" }
}
if ($NotesFile -and -not (Test-Path $NotesFile)) { throw "notes file not found: $NotesFile" }

# ---------------------------------------------------------------- 3. plan
$assets = @(Get-ChildItem $ArtifactsDir -File | Sort-Object Name | ForEach-Object { $_.FullName })
$createArgs = @('release', 'create', $Tag) + $assets + @('--repo', $Repo, '--title', "Riptide $Version", '--target', "$head")
if ($Stable) { $createArgs += '--latest' } else { $createArgs += '--prerelease' }
if ($NotesFile) { $createArgs += @('--notes-file', (Resolve-Path $NotesFile).Path) } else { $createArgs += '--generate-notes' }

Step 'Plan'
Write-Host "  1. gh release create $Tag ($(if ($Stable) { 'stable, --latest' } else { 'pre-release' })) with $($assets.Count) assets:"
$assets | ForEach-Object { Write-Host "       $(Split-Path -Leaf $_)" }
Write-Host '  2. ensure the pre-release channel-beta exists (create it if not)'
Write-Host "  3. gh release upload channel-beta latest-beta.json --clobber"
Write-Host "  4. scripts\verify-update-feed.ps1 -ExpectVersion $Version$(if ($Stable) { ' -Stable' })"

if ($DryRun) {
    Write-Host ''
    if ($script:refusals.Count -gt 0) { Write-Host "Dry run found $($script:refusals.Count) problem(s) that would stop -Publish." -ForegroundColor Yellow; exit 1 }
    Write-Host 'Dry run clean. Re-run with -Publish to publish.' -ForegroundColor Green
    exit 0
}

# ---------------------------------------------------------------- 4. publish
Step "Creating release $Tag"
$exists = Invoke-Native { & $ghPath release view $Tag --repo $Repo }
if ($exists.Code -eq 0) { throw "release $Tag already exists; refusing to overwrite it" }
$r = Invoke-Native { & $ghPath @createArgs }
$r.Output | ForEach-Object { Write-Host "  $_" }
if ($r.Code -ne 0) { throw 'gh release create failed' }

Step 'Beta channel (channel-beta)'
$view = Invoke-Native { & $ghPath release view channel-beta --repo $Repo }
if ($view.Code -ne 0) {
    $c = Invoke-Native { & $ghPath release create channel-beta --repo $Repo --prerelease --title 'Beta update channel' --notes 'Holds latest-beta.json for the beta update channel. Not a release.' }
    $c.Output | ForEach-Object { Write-Host "  $_" }
    if ($c.Code -ne 0) { throw 'cannot create the channel-beta release' }
}
$u = Invoke-Native { & $ghPath release upload channel-beta $betaPath --repo $Repo --clobber }
$u.Output | ForEach-Object { Write-Host "  $_" }
if ($u.Code -ne 0) { throw 'cannot upload latest-beta.json to channel-beta' }

Step 'Verifying the update feed'
$verify = Join-Path $PSScriptRoot 'verify-update-feed.ps1'
$verifyArgs = @{ ExpectVersion = $Version; Repo = $Repo }
if ($Stable) { $verifyArgs.Stable = $true }
& $verify @verifyArgs
if ($LASTEXITCODE -ne 0) {
    Write-Host 'The release is published but the feed check failed. See the runbook in CONTRIBUTING.md (if an update is bad).' -ForegroundColor Red
    exit 1
}
Write-Host ''
Write-Host "Published $Tag. Next: node scripts/set-release-version.mjs $Version, then open a PR for the site and README." -ForegroundColor Green
exit 0
