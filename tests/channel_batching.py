# python -I -B tests/channel_batching.py: #621, ST12. The starter channel's batching, run with a
# fake transport: a burst is one push, the owner's line goes first, and a write that fails loses
# nothing and marks nothing sent, so the next read of the same page sends it all once.
import importlib.util, os, sys, tempfile, types

KIT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'starter', 'claude-code')
os.environ['AGENTSHANGOUT_CHANNEL_BATCH'] = '8'
os.environ['AGENTSHANGOUT_CHANNEL_STATE'] = os.path.join(tempfile.mkdtemp(), '.channel_cursor')
sys.modules['ah'] = types.SimpleNamespace(call=lambda *a, **k: (False, '{}'))
spec = importlib.util.spec_from_file_location('channel', os.path.join(KIT, 'channel.py'))
ch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ch)

pushed, fail = [], [0]


def push(content, meta):
    if fail[0]:
        fail[0] -= 1
        raise OSError('broken pipe')
    pushed.append(meta)


ch.push = push
clock = [1000.0]
ch.time.monotonic = lambda: clock[0]


def event(cursor, mid, kind='mention', bot=True, owner=False, thread=None):
    data = {'hangout_id': 'R', 'message_id': mid, 'speaker': f'S{mid}', 'is_bot': bot,
            'from_your_owner': owner, 'body': 'text'}
    if thread:
        data['thread_id'] = thread
    return {'name': kind, 'eventId': f'evt_{cursor}', 'cursor': str(cursor), 'data': data}


held, seen, cursor = ch.Held(), set(), '10'
page = {'events': [event(11, 1, thread=5), event(12, 2, thread=5),
                   event(13, 3, kind='message', bot=False, owner=True)], 'cursor': '13'}

# Two bots' mentions, then the owner's "stop", and the write fails.
fail[0] = 1
try:
    ch.handle(page, cursor, seen, held)
    raise SystemExit('a failed write should stop handle')
except OSError:
    pass
assert pushed == [] and 3 not in seen, (pushed, seen)
assert ch.load_cursor() in (None, '10'), ch.load_cursor()
assert [len(b['events']) for b in held.batches.values()] == [2], held.batches

# The same page again: everything goes once, the owner's line first.
cursor = ch.handle(page, cursor, seen, held)
assert [(m['kind'], m.get('batch')) for m in pushed] == [('message', None), ('mention', '2')], pushed
assert cursor == '13' and ch.load_cursor() == '13' and {1, 2, 3} <= seen

# A batch due on time whose write fails stays held, the cursor stays before it, and it goes once.
pushed.clear()
cursor = ch.handle({'events': [event(14, 4)], 'cursor': '14'}, cursor, seen, held)
assert pushed == [] and ch.load_cursor() == '13'
clock[0] += 9
fail[0] = 1
try:
    ch.handle({'events': [], 'cursor': '14'}, cursor, seen, held)
except OSError:
    pass
assert pushed == [] and held.batches and ch.load_cursor() == '13'
cursor = ch.handle({'events': [], 'cursor': '14'}, cursor, seen, held)
assert len(pushed) == 1 and 'batch' not in pushed[0] and not held.batches
assert ch.load_cursor() == '14' and 4 in seen

# The owner's line is written but the batch after it fails: the retry sends only the batch.
pushed.clear()
page = {'events': [event(15, 5, thread=6), event(16, 6, kind='message', bot=False, owner=True)],
        'cursor': '16'}


def fail_second(content, meta, calls=[0]):
    calls[0] += 1
    if calls[0] == 2:
        raise OSError('broken pipe')
    pushed.append(meta)


ch.push = fail_second
try:
    ch.handle(page, cursor, seen, held)
    raise SystemExit('the failed batch write should stop handle')
except OSError:
    pass
assert [m['kind'] for m in pushed] == ['message'] and 6 in seen and 5 not in seen, (pushed, seen)
ch.push = push
cursor = ch.handle(page, cursor, seen, held)
assert [m['kind'] for m in pushed] == ['message'], 'the owner line once, not again: %s' % pushed
assert held.batches and ch.load_cursor() == '14', 'the mention still waits, cursor before it'
clock[0] += 9
cursor = ch.handle({'events': [], 'cursor': '16'}, cursor, seen, held)
assert [m['kind'] for m in pushed] == ['message', 'mention'], pushed
assert ch.load_cursor() == '16' and 5 in seen

# A mention already sent isn't sent again when its page comes back.
pushed.clear()
ch.handle({'events': [event(14, 4)], 'cursor': '14'}, cursor, seen, held)
clock[0] += 9
ch.handle({'events': [], 'cursor': '14'}, cursor, seen, held)
assert pushed == [], pushed
print('channel batching: bursts, owner first, failed writes lose nothing')
