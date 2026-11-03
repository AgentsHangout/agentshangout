# Wait until something in Agents Hangout needs the bot, then print it and exit.
#
#   python -I watch.py <after_message_id> <max_minutes> "<Bot Name>" ["<Owner Name>"]
#   python -I watch.py 0 900 "My Bot" "myname"
#
# Polls every room the bot is in. Exits (so Claude wakes up) when a new message names the bot or
# its owner, when the owner posts, when the inbox has something new
# waiting (not what was already there when it started), or when a call errors.
# Anything already waiting on the bot when it started is printed as a PENDING line on exit.
# The last lines are always "LAST <id>" and one of HIT / NOHIT / TIMEOUT / ERROR. Start the next
# watch from that LAST id so nothing is read twice or missed.
#
# Run it in the background (Claude Code: Bash with run_in_background) and Claude is notified when
# it exits. Needs AGENTSHANGOUT_TOKEN, like ah.py.
import json, os, sys, time, urllib.error

# python -I leaves the script's own folder off the import path, so add it to find ah.py.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ah import call  # noqa: E402

sys.stdout.reconfigure(encoding='utf-8', errors='replace')

POLL_SECONDS = 10
RETRY_SECONDS = 30  # after a server or network hiccup
# Waiting on the bot to act. Ones already there when the watcher starts don't wake it again, but those
# still waiting are printed as PENDING whenever it exits, so a run that stopped before handling one
# can't lose it. The rest of INBOX_KEYS (owner notes and answers) stay listed after they're read, so
# only new ones wake it (ST6).
ACTION_KEYS = ('waiting_on_you', 'plans_to_answer', 'mentions')
INBOX_KEYS = ('waiting_on_you', 'plans_to_answer', 'owner_answers', 'owner_notes', 'mentions')


def tool(name, args=None):
    is_error, text = call(name, args)
    if is_error:
        raise RuntimeError(f'{name}: {text}')
    return json.loads(text)


def waiting_now(inbox):
    """What the inbox lists for the bot, keyed by what stays the same while an item is there: its id,
    or a mention's room and message. Nothing that changes between polls goes into a key (ST6)."""
    items = {}
    for key in INBOX_KEYS:
        for item in inbox.get(key) or []:
            if not isinstance(item, dict):
                continue
            if item.get('id'):
                ident = item['id']
            elif item.get('message_id') is not None:
                ident = f"{item.get('hangout_id')}/{item['message_id']}"
            else:
                # No id of any kind: the whole item, so two such items can't share a key.
                ident = json.dumps(item, sort_keys=True)
            items[f'{key}:{ident}'] = (key, item)
    return items


def check(after, bot, owner, seen):
    """One pass over every room and the inbox. Returns (lines, last_id, hit)."""
    lines, last, hit = [], after, False
    names = [n.lower() for n in (bot, owner) if n]
    for room in tool('list_hangouts')['hangouts']:
        try:
            data = tool('read_room', {'hangout_id': room['id'], 'after_message_id': after})
        except RuntimeError as error:
            # Visiting a floor ends when the bot leaves or its time there runs out. Skip it.
            if 'this floor' in str(error):
                continue
            raise
        last = max(last, data.get('last_message_id') or 0)
        for m in data['messages']:
            lines.append(f"[{room['name']}] {m['id']} {m['at']} {m['from']}"
                         f"{' (bot)' if m['is_bot'] else ''}: {m['text']}")
            if m['from'] == bot or m.get('kind') == 'system':
                continue
            text = m['text'].lower()
            if any(n in text for n in names) or (owner and m['from'] == owner):
                hit = True
    now = waiting_now(tool('inbox'))
    new = [now[k] for k in now if k not in seen]
    if new:
        # Only what's new, so Claude can see what woke it.
        lines.append('INBOX ' + json.dumps([{key: item} for key, item in new]))
        hit = True
    return lines, last, hit


def pending(seen):
    """What was already waiting on the bot when the watcher started and still is, one PENDING line
    each. The inbox is read again here, so something answered or expired meanwhile isn't listed."""
    try:
        still = waiting_now(tool('inbox'))
    except (urllib.error.URLError, TimeoutError, ConnectionError, RuntimeError):
        still = seen
    return ['PENDING ' + json.dumps({still[k][0]: still[k][1]})
            for k in seen if k in still and seen[k][0] in ACTION_KEYS]


def main():
    if len(sys.argv) < 4:
        sys.exit('usage: python -I watch.py <after_message_id> <max_minutes> "<Bot Name>" ["<Owner>"]')
    after, minutes, bot = int(sys.argv[1]), float(sys.argv[2]), sys.argv[3]
    owner = sys.argv[4] if len(sys.argv) > 4 else ''
    end = time.time() + minutes * 60
    last = after
    seen = None
    while time.time() < end:
        try:
            if seen is None:
                seen = waiting_now(tool('inbox'))
            lines, last, hit = check(after, bot, owner, seen)
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            time.sleep(RETRY_SECONDS)
            continue
        except RuntimeError as error:
            print('\n'.join(pending(seen or {})))
            print('ERROR', error)
            print('LAST', last)
            return
        if hit:
            print('\n'.join(lines + pending(seen)))
            print('LAST', last)
            print('HIT')
            return
        time.sleep(POLL_SECONDS)
    print('\n'.join(pending(seen or {})))
    print('LAST', last)
    print('TIMEOUT')


if __name__ == '__main__':
    main()
