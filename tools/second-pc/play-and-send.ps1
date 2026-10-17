# Second PC: play with a session log (like PLAY-WITH-LOG.bat), then send every log and crash report not sent yet to
# the main PC's shared folder (\\<main PC>\SkateSessions\<this PC's name>\). A log that cannot be sent (main PC off)
# stays in logs\ and goes with the next send. First run asks for the main PC's name and the share account's password
# (saved in Windows Credential Manager, never in a file).
# Put it with PLAY-AND-SEND.bat next to skate3rust.exe. Guide: docs/hails-additions/second-pc-testing.md
param(
    # Only send what is pending, without playing.
    [switch]$SendOnly,
    # Ask for the main PC and password again.
    [switch]$Setup,
    # Check the GitHub fallback: send one small test file as a pull request (no game, share not used).
    [switch]$GitHubTest
)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$configFile = Join-Path $PSScriptRoot 'send-sessions.txt'
$sentFile = Join-Path $PSScriptRoot 'logs\sent.txt'
$crashDir = Join-Path $env:LOCALAPPDATA 'Skate3RustEngine\CrashReports'
New-Item -ItemType Directory -Force -Path (Join-Path $PSScriptRoot 'logs') | Out-Null

function Read-Config {
    if (-not (Test-Path $configFile)) { return $null }
    $c = @{}
    foreach ($line in Get-Content $configFile) { if ($line -match '^\s*(\w+)\s*=\s*(.*?)\s*$') { $c[$Matches[1]] = $Matches[2] } }
    $c
}

function Invoke-Setup {
    Write-Host 'One-time setup: where should session logs go?'
    $mainPc = Read-Host "Main PC's name (Settings > System > About on the main PC, e.g. MY-DESKTOP) or its IP address"
    $share = Read-Host 'Share name [SkateSessions]'
    if (-not $share) { $share = 'SkateSessions' }
    $account = Read-Host 'Account on the main PC [skatelogs]'
    if (-not $account) { $account = 'skatelogs' }
    $user = if ($mainPc -match '^\d+(\.\d+){3}$') { $account } else { "$mainPc\$account" }
    Write-Host "Saving the login for $mainPc in Windows Credential Manager; type the password you chose on the main PC."
    & cmdkey.exe "/add:$mainPc" "/user:$user" /pass
    if ($LASTEXITCODE -ne 0) { throw 'cmdkey could not save the login.' }
    $github = ''
    $answer = Read-Host 'When the main PC cannot be reached, send the logs as a pull request on GitHub instead? (needs the GitHub CLI, signed in; the repo is PUBLIC) [y/N]'
    if ($answer -match '^[yY]') {
        $github = Read-Host 'Repository [Hailey-Ross/rusty-trucks]'
        if (-not $github) { $github = 'Hailey-Ross/rusty-trucks' }
        # A missing gh must only warn: with ErrorActionPreference Stop it would end setup before the config is saved.
        $signedIn = $false
        if (Get-Command gh -ErrorAction SilentlyContinue) { & gh auth status 2>&1 | Out-Null; $signedIn = $LASTEXITCODE -eq 0 }
        if (-not $signedIn) { Write-Host 'The GitHub CLI is missing or not signed in: winget install GitHub.cli, then gh auth login.' }
    }
    "main_pc=$mainPc`nshare=$share`naccount=$user`ngithub_repo=$github`ngithub_base=hails-additions" |
        Set-Content -Encoding ASCII $configFile
    $target = "\\$mainPc\$share"
    if (Test-Path $target) { Write-Host "OK: $target is reachable." }
    else { Write-Host "Could not reach $target yet (main PC off, wrong name or password?). Logs stay here until it works." }
}

function Send-Pending {
    $c = Read-Config
    if (-not $c) { Write-Host 'Not set up: run with -Setup.'; return }
    $target = "\\$($c.main_pc)\$($c.share)\$env:COMPUTERNAME"
    $sent = @{}
    if (Test-Path $sentFile) { foreach ($n in Get-Content $sentFile) { $sent[$n] = $true } }
    $files = @(Get-ChildItem (Join-Path $PSScriptRoot 'logs') -File -Filter 'game-*.log')
    if (Test-Path $crashDir) { $files += @(Get-ChildItem $crashDir -File) }
    $pending = @($files | Where-Object { -not $sent[$_.FullName] })
    if ($pending.Count -eq 0) { Write-Host 'Nothing new to send.'; return }
    try {
        New-Item -ItemType Directory -Force -Path $target | Out-Null
        foreach ($f in $pending) {
            $dest = if ($f.DirectoryName -eq $crashDir) { Join-Path $target 'CrashReports' } else { $target }
            New-Item -ItemType Directory -Force -Path $dest | Out-Null
            Copy-Item -LiteralPath $f.FullName -Destination $dest -Force
            Add-Content -Path $sentFile -Value $f.FullName
            Write-Host "Sent $($f.Name)"
        }
        Write-Host "All sent to $target"
    } catch {
        Write-Host "Could not send to $target ($($_.Exception.Message))."
        $left = @($pending | Where-Object { -not (Select-String -Path $sentFile -SimpleMatch $_.FullName -Quiet -ErrorAction SilentlyContinue) })
        if ($c.github_repo -and $left.Count -gt 0) { Send-ToGitHub $c $left }
        else { Write-Host 'The logs stay in logs\ and go with the next send.' }
    }
}

# Fallback: zip the files, put the zip on a new branch session-logs/<PC>-<time> through the GitHub API (no clone
# needed) and open a pull request; the main PC fetches it (fetch-github-sessions.ps1) and closes it.
function Send-ToGitHub($c, $files) {
    $repo = $c.github_repo
    $base = if ($c.github_base) { $c.github_base } else { 'hails-additions' }
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $branch = "session-logs/$($env:COMPUTERNAME.ToLower())-$stamp"
    $work = Join-Path $env:TEMP "skate-session-$stamp"
    try {
        New-Item -ItemType Directory -Force -Path $work | Out-Null
        $zip = Join-Path $work "sessions-$($env:COMPUTERNAME)-$stamp.zip"
        Compress-Archive -LiteralPath ($files | ForEach-Object FullName) -DestinationPath $zip -CompressionLevel Optimal
        if ((Get-Item $zip).Length -gt 90MB) { throw 'the zip is over 90 MB, too big for the GitHub API' }
        $sha = & gh api "repos/$repo/git/ref/heads/$base" --jq '.object.sha'
        if ($LASTEXITCODE -ne 0 -or -not $sha) { throw "cannot read branch $base of $repo (gh signed in?)" }
        & gh api -X POST "repos/$repo/git/refs" -f "ref=refs/heads/$branch" -f "sha=$sha" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "cannot create branch $branch" }
        $body = Join-Path $work 'body.json'
        @{ message = "Session logs from $env:COMPUTERNAME ($stamp)"; branch = $branch
           content = [Convert]::ToBase64String([IO.File]::ReadAllBytes($zip)) } | ConvertTo-Json | Set-Content -Encoding ASCII $body
        & gh api -X PUT "repos/$repo/contents/session-logs/$(Split-Path $zip -Leaf)" --input $body | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'cannot upload the zip' }
        $names = ($files | ForEach-Object { "- $($_.Name)" }) -join "`n"
        $url = & gh pr create --repo $repo --base $base --head $branch --title "Session logs from $env:COMPUTERNAME ($stamp)" `
            --body "Session logs sent from $env:COMPUTERNAME because the main PC could not be reached. Not for merging: the main PC fetches the zip and closes this.`n`n$names"
        if ($LASTEXITCODE -ne 0) { throw 'cannot open the pull request' }
        foreach ($f in $files) { Add-Content -Path $sentFile -Value $f.FullName }
        Write-Host "Sent $($files.Count) file(s) to GitHub instead: $url"
    } catch {
        Write-Host "GitHub fallback failed too ($($_.Exception.Message)). The logs stay in logs\ and go with the next send."
    } finally {
        Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
    }
}

if ($GitHubTest) {
    $c = Read-Config
    if (-not $c -or -not $c.github_repo) { throw 'The GitHub fallback is not set up: run with -Setup and answer y.' }
    $test = Join-Path $env:TEMP "github-fallback-test-$env:COMPUTERNAME.txt"
    "GitHub fallback test from $env:COMPUTERNAME at $(Get-Date -Format s)" | Set-Content $test
    $sentFile = Join-Path $env:TEMP 'github-fallback-test-sent.txt'
    Send-ToGitHub $c @(Get-Item $test)
    Remove-Item $test, $sentFile -ErrorAction SilentlyContinue
    return
}
if ($Setup -or -not (Read-Config)) { Invoke-Setup }
if (-not $SendOnly) {
    if (-not (Test-Path (Join-Path $PSScriptRoot 'skate3rust.exe'))) { throw 'skate3rust.exe is not in this folder.' }
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $log = "logs\game-$stamp.log"
    $errLog = "logs\game-$stamp.stderr.log"
    Write-Host "Playing with logging: $PSScriptRoot\$errLog"
    Start-Process -FilePath '.\skate3rust.exe' -RedirectStandardOutput $log -RedirectStandardError $errLog -Wait
    Write-Host 'Session finished.'
}
Send-Pending
