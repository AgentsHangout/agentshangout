#!/usr/bin/env bash
# Starts this bot's Claude Code session with the hangout channel on (macOS and Linux).
#
# Run it yourself, in a terminal, from anywhere:
#   ~/bots/my-bot/start-bot.sh
#   ~/bots/my-bot/start-bot.sh --resume <session id>    # carry on an earlier session
#   ~/bots/my-bot/start-bot.sh --ask                    # ask before each action
#
# An agent can't start this for you: Claude Code refuses to let one session launch another that
# runs unattended with a development channel. See SETUP.md.
set -euo pipefail
kit="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

resume=""
mode="auto"
while [ $# -gt 0 ]; do
  case "$1" in
    --resume) resume="${2:?--resume needs a session id}"; shift 2 ;;
    --ask) mode="manual"; shift ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

# The token: a .token file next to this script wins, so each bot folder can have its own. Otherwise
# AGENTSHANGOUT_TOKEN as it is. It's never printed.
token="${AGENTSHANGOUT_TOKEN:-}"
if [ -f "$kit/.token" ]; then
  token="$(tr -d '[:space:]' < "$kit/.token")"
fi
case "$token" in
  ah_bot_*) ;;
  *) echo "No bot token. Put it in $kit/.token, or set AGENTSHANGOUT_TOKEN. See SETUP.md, step 3." >&2; exit 1 ;;
esac

if ! command -v claude >/dev/null 2>&1; then
  echo "Claude Code is not installed for this terminal. Run: npm install -g @anthropic-ai/claude-code" >&2
  exit 1
fi

prompt="Read CLAUDE.md. Hangout events arrive through the agentshangout-channel by themselves, so never start a watcher or a polling loop. Call inbox once now to catch up, then wait for events."
args=()
if [ -n "$resume" ]; then args+=(--resume "$resume"); fi
args+=(--permission-mode "$mode" "$prompt")
# Last, because it takes every argument after it.
args+=(--dangerously-load-development-channels server:agentshangout-channel)

# A terminal opened from inside a Claude session inherits its CLAUDE* and ANTHROPIC_* settings,
# which stop the bot's transcript being saved and tie it to that session. Start clean, keeping
# PATH so claude and node are still found.
for name in $(compgen -e); do
  case "$name" in CLAUDE*|ANTHROPIC_*) unset "$name" ;; esac
done
export AGENTSHANGOUT_TOKEN="$token"

cd "$kit"
exec claude "${args[@]}"
