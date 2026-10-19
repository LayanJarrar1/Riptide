#Requires -Version 5.1
<#
.SYNOPSIS
    Proves that people with an older Riptide will be told about a release.

.DESCRIPTION
    Does what the app does, without any credentials: plain HTTPS GETs of the public update
    feed, then checks that
      * the JSON parses and has version (semver), notes, pub_date and
        platforms.windows-x86_64.url + signature;
      * the installer URL answers 200 (redirects followed);
      * the feed version equals -ExpectVersion, when given.
    It never sends a GitHub token. Exits 1 on any problem, 0 when the feed is good.

    The feeds (see apps/desktop/src-tauri/src/updater.rs):
      beta    https://github.com/<repo>/releases/download/channel-beta/latest-beta.json
      stable  https://github.com/<repo>/releases/latest/download/latest.json   (-Stable)

.PARAMETER ExpectVersion
    The version the feed must announce, e.g. 0.1.0-alpha.5.
.PARAMETER Stable
    Also check the stable feed (only stable releases carry latest.json).
.PARAMETER Repo
    owner/name. Default LayanJarrar1/Riptide.
.PARAMETER BetaFeedUrl
    Override the beta feed URL (for testing the script against a local server).

.EXAMPLE
    .\scripts\verify-update-feed.ps1 -ExpectVersion 0.1.0-alpha.5
#>
[CmdletBinding()]
param(
    [string]$ExpectVersion = '',
    [switch]$Stable,
    [string]$Repo = 'LayanJarrar1/Riptide',
    [string]$BetaFeedUrl = ''
)

$ErrorActionPreference = 'Stop'
try { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 } catch { }

$semver = '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$'
$script:problems = New-Object System.Collections.Generic.List[string]

function Fail([string]$Text) { $script:problems.Add($Text); Write-Host "  FAIL  $Text" -ForegroundColor Red }
function Pass([string]$Text) { Write-Host "  ok    $Text" -ForegroundColor Green }

function Get-Text([string]$Url) {
    $r = Invoke-WebRequest -Uri $Url -UseBasicParsing -MaximumRedirection 10 -Headers @{ 'User-Agent' = 'riptide-verify-update-feed' }
    $c = $r.Content
    if ($c -is [byte[]]) { $c = [Text.Encoding]::UTF8.GetString($c) }
    return $c.TrimStart([char]0xFEFF)
}

# Status of an installer URL: HEAD first, then a one-byte ranged GET (some CDNs refuse HEAD).
function Test-Url([string]$Url) {
    $h = @{ 'User-Agent' = 'riptide-verify-update-feed' }
    try {
        $r = Invoke-WebRequest -Uri $Url -Method Head -UseBasicParsing -MaximumRedirection 10 -Headers $h
        return [int]$r.StatusCode
    } catch {
        try {
            $h['Range'] = 'bytes=0-0'
            $r = Invoke-WebRequest -Uri $Url -Method Get -UseBasicParsing -MaximumRedirection 10 -Headers $h
            if ([int]$r.StatusCode -eq 206) { return 200 }
            return [int]$r.StatusCode
        } catch {
            if ($_.Exception.Response) { return [int]$_.Exception.Response.StatusCode }
            return 0
        }
    }
}

function Test-Feed([string]$Name, [string]$Url, [string]$Expect) {
    Write-Host ''
    Write-Host "$Name feed: $Url" -ForegroundColor Cyan
    try { $text = Get-Text $Url } catch { Fail "${Name}: cannot fetch the feed ($($_.Exception.Message))"; return }
    try { $feed = $text | ConvertFrom-Json } catch { Fail "${Name}: the feed is not valid JSON"; return }

    if ($feed.version -isnot [string] -or $feed.version -notmatch $semver) { Fail "${Name}: version '$($feed.version)' is not semver" }
    else { Pass "version $($feed.version)" }
    if (-not $feed.notes -or -not "$($feed.notes)".Trim()) { Fail "${Name}: notes are empty" } else { Pass 'notes present' }
    $date = [DateTime]::MinValue
    if (-not $feed.pub_date -or -not [DateTime]::TryParse("$($feed.pub_date)", [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AdjustToUniversal, [ref]$date)) { Fail "${Name}: pub_date is missing or not a date" }
    else { Pass "pub_date $($feed.pub_date)" }

    $win = $null
    if ($feed.platforms) { $win = $feed.platforms.'windows-x86_64' }
    if ($null -eq $win) { Fail "${Name}: platforms.windows-x86_64 is missing"; return }
    if (-not $win.signature -or -not "$($win.signature)".Trim()) { Fail "${Name}: windows-x86_64 signature is empty" } else { Pass 'signature present' }
    if (-not $win.url -or "$($win.url)" -notmatch '^https://') { Fail "${Name}: windows-x86_64 url is missing or not https"; return }

    $code = Test-Url "$($win.url)"
    if ($code -eq 200) { Pass "installer answers 200: $($win.url)" } else { Fail "${Name}: installer URL answered ${code}: $($win.url)" }

    if ($Expect) {
        if ($feed.version -eq $Expect) { Pass "feed announces the expected version $Expect" }
        else { Fail "${Name}: feed announces '$($feed.version)', expected '$Expect'" }
    }
}

$betaUrl = if ($BetaFeedUrl) { $BetaFeedUrl } else { "https://github.com/$Repo/releases/download/channel-beta/latest-beta.json" }
Test-Feed 'beta' $betaUrl $ExpectVersion
if ($Stable) {
    # a pre-release is never "latest", so the stable feed only moves for plain versions
    $stableExpect = if ($ExpectVersion -and $ExpectVersion -notmatch '-') { $ExpectVersion } else { '' }
    Test-Feed 'stable' "https://github.com/$Repo/releases/latest/download/latest.json" $stableExpect
}

Write-Host ''
if ($script:problems.Count -gt 0) {
    Write-Host "$($script:problems.Count) problem(s): users on older versions will NOT be notified until this is fixed." -ForegroundColor Red
    exit 1
}
Write-Host 'The update feed is good: people on an older version will be notified.' -ForegroundColor Green
exit 0
