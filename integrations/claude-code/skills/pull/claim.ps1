# Context Drop - deterministic claim for Windows, run by the skill loader (`!`
# injection) the moment /context-drop:pull or /cd expands, BEFORE the model
# sees the user's instruction. Windows counterpart of claim.sh: same steps,
# same output. The installer writes this invocation into SKILL.md on Windows:
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File "<skill dir>/claim.ps1" <session-id>
#
# That command line means the same thing from Git Bash, PowerShell, and cmd,
# so it works whichever shell Claude Code uses for the injection.
#
# Prints metadata only (never packet content):
#   CONTEXT_DROP_BIN=<resolved binary>
#   CLAIM_EXIT=<claim exit code>
#   <claim --json output>
#   PROCESSING_EXIT=<processing exit code>   (only when the claim succeeded)
#
# Keep this file ASCII-only: Windows PowerShell 5.1 reads BOM-less files in the
# legacy code page.

param([string]$SessionId = "")

# The CLI writes UTF-8. Decode (and re-emit) it as UTF-8 instead of the console
# code page, or a non-ASCII path (e.g. CP932 + a Japanese user name) corrupts
# the claim JSON and processing is silently skipped.
$utf8 = New-Object System.Text.UTF8Encoding($false)
try { [Console]::OutputEncoding = $utf8 } catch { }
$OutputEncoding = $utf8

# Step 0 - resolve the binary: $CONTEXT_DROP_BIN -> canonical managed copy -> PATH.
$bin = $env:CONTEXT_DROP_BIN
if (-not $bin) {
    $candidates = @()
    if ($env:CONTEXT_DROP_DATA_DIR) {
        $candidates += (Join-Path $env:CONTEXT_DROP_DATA_DIR 'bin\context-drop.exe')
        $candidates += (Join-Path $env:CONTEXT_DROP_DATA_DIR 'bin\context-drop')
    }
    if ($env:APPDATA) {
        $candidates += (Join-Path $env:APPDATA 'com.contextdrop.app\bin\context-drop.exe')
    }
    foreach ($c in $candidates) {
        if (Test-Path -LiteralPath $c -PathType Leaf) { $bin = $c; break }
    }
}
if (-not $bin) {
    $cmd = Get-Command context-drop -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($cmd) { $bin = $cmd.Source }
}
if (-not $bin) {
    Write-Output 'CONTEXT_DROP_BIN='
    Write-Output 'CLAIM_EXIT=127'
    Write-Output '{"ok":false,"error":"NOT_INSTALLED","message":"context-drop CLI not found"}'
    exit 0
}
Write-Output "CONTEXT_DROP_BIN=$bin"

$sessionArgs = @()
if ($SessionId) { $sessionArgs = @('--session-id', $SessionId) }

$out = (& $bin claim --json @sessionArgs | Out-String).TrimEnd()
$code = $LASTEXITCODE
Write-Output "CLAIM_EXIT=$code"
Write-Output $out

# Mark PROCESSING right away so the claimed packet is protected from TTL
# cleanup while the subagent investigates.
if ($code -eq 0) {
    $claim = $null
    try { $claim = $out | ConvertFrom-Json } catch { $claim = $null }
    if ($claim -and $claim.packetId -and $claim.claimId) {
        & $bin processing $claim.packetId --claim-id $claim.claimId @sessionArgs *> $null
        Write-Output "PROCESSING_EXIT=$LASTEXITCODE"
    }
}
exit 0
