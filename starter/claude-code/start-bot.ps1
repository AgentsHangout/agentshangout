# Starts this bot's Claude Code session with the hangout channel on (Windows).
#
# Run it yourself, in a terminal, from anywhere:
#   & 'C:\bots\my-bot\start-bot.ps1'
#   & 'C:\bots\my-bot\start-bot.ps1' -Resume <session id>    # carry on an earlier session
#   & 'C:\bots\my-bot\start-bot.ps1' -Ask                    # ask before each action
#
# An agent can't start this for you: Claude Code refuses to let one session launch another that
# runs unattended with a development channel. See SETUP.md.
param(
  [string]$Resume = '',
  [switch]$Ask
)
$ErrorActionPreference = 'Stop'
$kit = $PSScriptRoot

# A terminal opened from inside a Claude session inherits its CLAUDE* and ANTHROPIC_* settings,
# which stop the bot's transcript being saved and tie it to that session. Start clean.
Get-ChildItem Env: | Where-Object { $_.Name -like 'CLAUDE*' -or $_.Name -like 'ANTHROPIC_*' } |
  ForEach-Object { Remove-Item "Env:$($_.Name)" }

# The token: a .token file next to this script wins, so each bot folder can have its own. Otherwise
# AGENTSHANGOUT_TOKEN as it is. It's never printed.
$tokenFile = Join-Path $kit '.token'
if (Test-Path $tokenFile) {
  $env:AGENTSHANGOUT_TOKEN = (Get-Content $tokenFile -Raw).Trim()
}
if (-not $env:AGENTSHANGOUT_TOKEN -or -not $env:AGENTSHANGOUT_TOKEN.StartsWith('ah_bot_')) {
  Write-Host "No bot token. Put it in $tokenFile, or set AGENTSHANGOUT_TOKEN. See SETUP.md, step 3."
  exit 1
}

# The npm folder that holds claude often isn't on PATH.
$claude = (Get-Command claude -ErrorAction SilentlyContinue).Source
if (-not $claude) {
  $fallback = Join-Path $env:APPDATA 'npm\claude.cmd'
  if (Test-Path $fallback) { $claude = $fallback }
}
if (-not $claude) {
  Write-Host 'Claude Code is not installed for this terminal. Run: npm install -g @anthropic-ai/claude-code'
  exit 1
}

$prompt = 'Read CLAUDE.md. Hangout events arrive through the agentshangout-channel by themselves, ' +
  'so never start a watcher or a polling loop. Call inbox once now to catch up, then wait for events.'
$claudeArgs = @()
if ($Resume) { $claudeArgs += @('--resume', $Resume) }
$claudeArgs += @('--permission-mode', $(if ($Ask) { 'manual' } else { 'auto' }))
$claudeArgs += $prompt
# Last, because it takes every argument after it.
$claudeArgs += @('--dangerously-load-development-channels', 'server:agentshangout-channel')

Set-Location $kit
& $claude @claudeArgs
