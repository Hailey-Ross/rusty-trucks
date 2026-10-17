# Main PC: fetch session logs a second PC sent as pull requests (play-and-send.ps1's GitHub fallback). Every open PR
# whose branch starts with session-logs/ is downloaded and unpacked into <Folder>\<second PC name>\, then closed and its
# branch deleted. Needs the GitHub CLI, signed in.
param(
    # The shared session folder (the one setup-main-pc.ps1 shared).
    [Parameter(Mandatory)][string]$Folder,
    [string]$Repo = 'Hailey-Ross/rusty-trucks'
)
$ErrorActionPreference = 'Stop'
$prs = & gh pr list --repo $Repo --state open --limit 100 --json number,headRefName | ConvertFrom-Json |
    Where-Object { $_.headRefName -like 'session-logs/*' }
if (-not $prs) { Write-Host 'FETCH_DONE no session-log pull requests open.'; return }
foreach ($pr in $prs) {
    $branch = $pr.headRefName
    # session-logs/<pc>-<yyyyMMdd>-<HHmmss>
    $pc = ($branch -replace '^session-logs/', '') -replace '-\d{8}-\d{6}$', ''
    $dest = Join-Path $Folder $pc.ToUpper()
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    $items = & gh api "repos/$Repo/contents/session-logs?ref=$branch" | ConvertFrom-Json
    foreach ($item in $items | Where-Object { $_.name -like '*.zip' }) {
        $zip = Join-Path $env:TEMP $item.name
        Invoke-WebRequest -UseBasicParsing -Uri $item.download_url -OutFile $zip
        Expand-Archive -LiteralPath $zip -DestinationPath $dest -Force
        Remove-Item $zip
        Write-Host "PR #$($pr.number): unpacked $($item.name) into $dest"
    }
    & gh pr close $pr.number --repo $Repo --delete-branch --comment 'Fetched to the main PC.' | Out-Null
    Write-Host "PR #$($pr.number): closed, branch $branch deleted."
}
Write-Host 'FETCH_DONE'
