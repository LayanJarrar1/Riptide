#Requires -Version 5.1
<#
.SYNOPSIS
    Riptide's merge gate. Everything here must be green before a PR merges.

.DESCRIPTION
    Runs, in order:
      desktop   : npm ci (if needed), i18n lint, ng lint, ng build (production), ng test (once, headless)
      rust      : cargo fmt --check, cargo build --release, cargo clippy --all-targets -D warnings, cargo test
      extension : tsc --noEmit + eslint (M2)
    The Angular build runs before cargo because tauri-build requires the frontend bundle to exist.
    Parts that do not exist yet are reported as SKIPPED, loudly, never silently.

.PARAMETER Quick
    Skip the release build (debug build still happens through clippy/test). For local loops only;
    CI always runs the full gate.
.PARAMETER SkipWeb
    Skip the Angular and extension steps (Rust only).
.PARAMETER SkipRust
    Skip the cargo steps (web only).
#>
[CmdletBinding()]
param(
    [switch]$Quick,
    [switch]$SkipWeb,
    [switch]$SkipRust
)

$ErrorActionPreference = 'Stop'
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

# cargo lives in ~/.cargo/bin; make sure it is reachable even from a fresh shell
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if ((Test-Path $cargoBin) -and ($env:PATH -notlike "*$cargoBin*")) { $env:PATH = "$cargoBin;$env:PATH" }

$script:Failures = New-Object System.Collections.Generic.List[string]
$script:Skipped  = New-Object System.Collections.Generic.List[string]
$script:Passed   = New-Object System.Collections.Generic.List[string]
$total = [System.Diagnostics.Stopwatch]::StartNew()

function Invoke-Step {
    param([string]$Name, [scriptblock]$Body)
    Write-Host ''
    Write-Host "==> $Name" -ForegroundColor Cyan
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $global:LASTEXITCODE = 0
    # Native tools (cargo, npm) print progress on stderr. Under 'Stop', PowerShell 5.1 turns a
    # redirected stderr line into a terminating error, so the preference is relaxed while the
    # step runs; the exit code is what decides pass/fail.
    $savedPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $Body
        $code = $LASTEXITCODE
    } catch {
        Write-Host $_.Exception.Message -ForegroundColor Red
        $code = 1
    } finally {
        $ErrorActionPreference = $savedPreference
    }
    if ($null -eq $code) { $code = 0 }
    if ($code -ne 0) {
        $script:Failures.Add($Name)
        Write-Host ("FAILED  {0}  (exit {1}, {2:n1}s)" -f $Name, $code, $sw.Elapsed.TotalSeconds) -ForegroundColor Red
    } else {
        $script:Passed.Add($Name)
        Write-Host ("OK      {0}  ({1:n1}s)" -f $Name, $sw.Elapsed.TotalSeconds) -ForegroundColor Green
    }
}

function Skip-Step {
    param([string]$Name, [string]$Why)
    $script:Skipped.Add("$Name - $Why")
    Write-Host ''
    Write-Host "SKIPPED $Name - $Why" -ForegroundColor Yellow
}

# ---------------------------------------------------------------------------
# Desktop app (Angular). Runs first: tauri-build needs dist/ to exist.
# ---------------------------------------------------------------------------
$desktop = Join-Path $RepoRoot 'apps\desktop'
if ($SkipWeb) {
    Skip-Step 'desktop' '-SkipWeb'
} elseif (-not (Test-Path (Join-Path $desktop 'package.json'))) {
    Skip-Step 'desktop' 'apps/desktop/package.json does not exist yet (M0 desktop shell)'
} else {
    Push-Location $desktop
    try {
        if (-not (Test-Path 'node_modules')) {
            Invoke-Step 'desktop: npm ci' { npm ci --no-audit --no-fund }
        }
        Invoke-Step 'desktop: i18n lint'  { npm run --silent i18n:lint }
        Invoke-Step 'desktop: ng lint'    { npm run --silent lint }
        Invoke-Step 'desktop: ng build'   { npm run --silent build }
        Invoke-Step 'desktop: ng test'    { npm run --silent test }
    } finally {
        Pop-Location
    }
}

# ---------------------------------------------------------------------------
# Rust workspace
# ---------------------------------------------------------------------------
if ($SkipRust) {
    Skip-Step 'rust' '-SkipRust'
} elseif (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $script:Failures.Add('rust: cargo not found')
    Write-Host 'FAILED  cargo is not installed (https://rustup.rs)' -ForegroundColor Red
} else {
    Invoke-Step 'rust: cargo fmt --check' { cargo fmt --all -- --check }
    if ($Quick) {
        Skip-Step 'rust: cargo build --release' '-Quick'
    } else {
        Invoke-Step 'rust: cargo build --release' { cargo build --release --workspace }
    }
    Invoke-Step 'rust: cargo clippy -D warnings' { cargo clippy --workspace --all-targets -- -D warnings }
    # Doctests of a --workspace run inherit every native link-search dir in the workspace, and
    # tauri-build drops a stub msvcrt.lib (static VC runtime for the app binary) into its own
    # out dir; that stub shadows the real CRT for every other crate's doctest binary (LNK2001
    # __CxxFrameHandler3 / memcmp). Test the engine crates without the desktop crate in the
    # graph, then the desktop crate on its own.
    Invoke-Step 'rust: cargo test (engine crates)' { cargo test --workspace --exclude riptide-desktop }
    Invoke-Step 'rust: cargo test (desktop)' { cargo test -p riptide-desktop }
    # The webview's TypeScript types are generated from the Rust types (ts-rs). Regenerate and
    # fail if the committed bindings differ: the schema exists once, in Rust.
    Invoke-Step 'rust: ts bindings up to date' {
        $env:TS_RS_EXPORT_DIR = Join-Path $RepoRoot 'apps\desktop\src\app\tauri\bridge'
        cargo test -p riptide-core -p riptide-engine --features riptide-core/ts,riptide-engine/ts export_bindings
        if ($LASTEXITCODE -ne 0) { return }
        git diff --quiet --exit-code -- apps/desktop/src/app/tauri/bridge
        if ($LASTEXITCODE -ne 0) {
            git --no-pager diff --stat -- apps/desktop/src/app/tauri/bridge
            Write-Host 'Generated TypeScript bindings differ from the committed ones; commit the regenerated files.' -ForegroundColor Red
            cmd /c exit 1
        }
    }
}

# ---------------------------------------------------------------------------
# Browser extension (M2)
# ---------------------------------------------------------------------------
$extension = Join-Path $RepoRoot 'apps\extension'
if ($SkipWeb) {
    Skip-Step 'extension' '-SkipWeb'
} elseif (-not (Test-Path (Join-Path $extension 'package.json'))) {
    Skip-Step 'extension' 'apps/extension/package.json does not exist yet (M2)'
} else {
    Push-Location $extension
    try {
        if (-not (Test-Path 'node_modules')) {
            Invoke-Step 'extension: npm ci' { npm ci --no-audit --no-fund }
        }
        Invoke-Step 'extension: tsc --noEmit' { npx --no-install tsc --noEmit }
        Invoke-Step 'extension: eslint'       { npx --no-install eslint . }
    } finally {
        Pop-Location
    }
}

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host ('=' * 72)
Write-Host ("check.ps1 finished in {0:n0}s" -f $total.Elapsed.TotalSeconds)
foreach ($p in $script:Passed)   { Write-Host "  OK      $p" -ForegroundColor Green }
foreach ($s in $script:Skipped)  { Write-Host "  SKIPPED $s" -ForegroundColor Yellow }
foreach ($f in $script:Failures) { Write-Host "  FAILED  $f" -ForegroundColor Red }
Write-Host ('=' * 72)

if ($script:Failures.Count -gt 0) {
    Write-Host ("RED: {0} step(s) failed." -f $script:Failures.Count) -ForegroundColor Red
    exit 1
}
Write-Host 'GREEN.' -ForegroundColor Green
exit 0
