#!/bin/sh
# Runs on the server as root, from the Publish workflow, when the pages fail after a deploy
# (#576): puts back the binary deploy.sh saved before installing. The workflow looks for UNDO_OK.
#   undo.sh <sha256 of the binary being taken out>
# It only goes back if that binary is still the one installed, so a later deploy is never undone.
# The newest-installed tag stays as it was, so Publish still refuses anything older (see AGENTS.md).
set -eu

want="${1:?give the sha256 of the binary being taken out}"
case "$want" in
    *[!0-9a-f]*) echo "sha256 must be hex" >&2; exit 1 ;;
esac

bin=/usr/local/bin/agenthangout

exec 9> /run/agenthangout-deploy.lock
flock -n 9 || { echo "A deploy is running, so nothing was undone" >&2; exit 1; }

if ! echo "$want  $bin" | sha256sum -c --status -; then
    echo "The installed binary isn't the one to take out, so nothing was undone" >&2
    exit 1
fi
[ -f "$bin.previous" ] || { echo "There's no previous binary to go back to" >&2; exit 1; }

install -m 755 "$bin.previous" "$bin"
systemctl restart agenthangout
for _ in $(seq 1 30); do
    if curl -fsS -o /dev/null --max-time 2 http://127.0.0.1:8787/api/worlds/commons; then
        echo "UNDO_OK"
        exit 0
    fi
    sleep 1
done
echo "The previous binary didn't come up" >&2
exit 1
