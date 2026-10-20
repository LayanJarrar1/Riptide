#Requires -Version 5.1
<#
.SYNOPSIS
    Scans the repository (working tree and full git history) for committed secrets with gitleaks.

.DESCRIPTION
    Looks for gitleaks in this order: -Binary, $env:GITLEAKS, the PATH, then
    %USERPROFILE%\tools\gitleaks\gitleaks.exe. If none exists the scan is reported as SKIPPED,
    loudly, and the script exits 0 so the merge gate does not depend on a tool being installed.
    Findings are printed with secrets redacted (--redact); the exit code is non-zero when any
    secret is found. Rules and allowlists live in .gitleaks.toml at the repository root.

.PARAMETER Binary
    Path to a gitleaks executable to use instead of the search order above.

.PARAMETER WorkingTreeOnly
    Scan only the files on disk (gitleaks dir) and skip the git history.
#>
[CmdletBinding()]
param(
    [string]$Binary,
    [switch]$WorkingTreeOnly
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$config = Join-Path $repo '.gitleaks.toml'

$candidates = @()
if ($Binary) { $candidates += $Binary }
if ($env:GITLEAKS) { $candidates += $env:GITLEAKS }
$onPath = Get-Command gitleaks -ErrorAction SilentlyContinue
if ($onPath) { $candidates += $onPath.Source }
$candidates += (Join-Path $env:USERPROFILE 'tools\gitleaks\gitleaks.exe')

$gitleaks = $candidates | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } | Select-Object -First 1
if (-not $gitleaks) {
    Write-Host 'SKIPPED: gitleaks not found (install it, or set $env:GITLEAKS or pass -Binary).' -ForegroundColor Yellow
    exit 0
}
if (-not (Test-Path -LiteralPath $config -PathType Leaf)) {
    Write-Host "FAILED: missing $config" -ForegroundColor Red
    exit 1
}

$version = (& $gitleaks version 2>$null | Select-Object -First 1)
Write-Host "gitleaks $version ($gitleaks)"

$failed = $false
Push-Location $repo
try {
    & $gitleaks dir . --config $config --redact --no-banner --exit-code 1
    if ($LASTEXITCODE -ne 0) { $failed = $true; Write-Host 'working tree: secrets found' -ForegroundColor Red }
    else { Write-Host 'working tree: no leaks found' -ForegroundColor Green }

    if (-not $WorkingTreeOnly) {
        & $gitleaks git . --config $config --redact --no-banner --exit-code 1 --log-opts='--all'
        if ($LASTEXITCODE -ne 0) { $failed = $true; Write-Host 'git history: secrets found' -ForegroundColor Red }
        else { Write-Host 'git history (all refs): no leaks found' -ForegroundColor Green }
    }
}
finally {
    Pop-Location
}

if ($failed) { exit 1 }
exit 0
