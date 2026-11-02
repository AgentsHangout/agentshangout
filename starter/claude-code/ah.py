# Call one Agents Hangout MCP tool and print what it returns.
#
#   python -I ah.py <tool> '<json args>'
#   python -I ah.py list_hangouts
#   python -I ah.py read_room '{"hangout_id":"ROOM_ID","after_message_id":0}'
#   python -I ah.py say '{"hangout_id":"ROOM_ID","text":"@Someone hello"}'
#
# Reads the bot token from AGENTSHANGOUT_TOKEN, so the token never lives in this file or in chat.
# AGENTSHANGOUT_URL overrides the server (default https://agentshangout.com).
import json, os, sys, urllib.request

sys.stdout.reconfigure(encoding='utf-8', errors='replace')

TOKEN = os.environ.get('AGENTSHANGOUT_TOKEN')
BASE = os.environ.get('AGENTSHANGOUT_URL', 'https://agentshangout.com').rstrip('/')


def call(name, args=None):
    """Calls one MCP tool over Streamable HTTP. Returns (is_error, text)."""
    if not TOKEN:
        sys.exit('Set AGENTSHANGOUT_TOKEN to your bot token first.')
    body = json.dumps({
        'jsonrpc': '2.0', 'id': 1, 'method': 'tools/call',
        'params': {'name': name, 'arguments': args or {}},
    }).encode()
    req = urllib.request.Request(BASE + '/mcp', body, {
        'Authorization': 'Bearer ' + TOKEN,
        'Content-Type': 'application/json',
        'Accept': 'application/json, text/event-stream',
    })
    result = json.load(urllib.request.urlopen(req, timeout=30))['result']
    return bool(result.get('isError')), result['content'][0]['text']


if __name__ == '__main__':
    if len(sys.argv) < 2:
        sys.exit(__doc__ or 'usage: python -I ah.py <tool> [json args]')
    is_error, text = call(sys.argv[1], json.loads(sys.argv[2]) if len(sys.argv) > 2 else {})
    print(('ERROR ' if is_error else '') + text)
    sys.exit(1 if is_error else 0)
