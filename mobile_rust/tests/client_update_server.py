"""Loopback-only A/B deployment fixture for real-browser service-worker checks.

Run from the repo: python mobile_rust/tests/client_update_server.py
Uses the production worker and update controller. No account data or external services.
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import hashlib
import json
from urllib.parse import urlparse, parse_qs

ROOT = Path(__file__).resolve().parents[2]
revision = 'A'
hits = {}


def bundle():
    app = f"""import {{ createClientUpdates }} from './client-updates.js';
document.querySelector('#revision').textContent = '{revision}';
const updates = createClientUpdates(); window.updates = updates;
updates.subscribe(s => document.querySelector('#status').textContent = s.ready ? 'ready' : 'current');
document.querySelector('#refresh').onclick = () => updates.activate();
updates.start();
""".encode()
    files = {'app.js': app, 'client-updates.js': (ROOT / 'web/client-updates.js').read_bytes(),
             'client-entry.js': (ROOT / 'web/client-entry.js').read_bytes()}
    version = hashlib.sha256(b''.join(files.values())).hexdigest()
    manifest = {'version': version, 'files': [
        {'url': f'/client/{version}/{name}', 'sha256': hashlib.sha256(data).hexdigest()}
        for name, data in files.items()]}
    return version, files, manifest


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        global revision
        url = urlparse(self.path)
        path = url.path
        hits[path] = hits.get(path, 0) + 1
        version, files, manifest = bundle()
        mime, cache, status = 'text/javascript', 'no-cache', 200
        if path == '/__revision':
            value = parse_qs(url.query).get('value', ['A'])[0]
            if value not in ('A', 'B'):
                self.send_error(400)
                return
            revision = value
            data, mime = b'OK', 'text/plain'
        elif path == '/__stats':
            data, mime = json.dumps(hits).encode(), 'application/json'
        elif path == '/client-worker.js':
            data = ('const CLIENT_MANIFEST = ' + json.dumps(manifest) + ';\n').encode() + (ROOT / 'web/client-worker.js').read_bytes()
        elif path == '/':
            data = f'''<!doctype html><meta name="viewport" content="width=device-width"><title>Client update test</title>
<div id="app"><h1 id="revision"></h1><textarea id="draft" aria-label="Draft"></textarea>
<p id="status"></p><button id="refresh">Refresh app</button>
<script type="module" src="/client/{version}/client-entry.js"></script></div>'''.encode()
            mime = 'text/html'
        elif path.startswith(f'/client/{version}/') and path.rsplit('/', 1)[1] in files:
            data = files[path.rsplit('/', 1)[1]]
            cache = 'public, max-age=31536000, immutable'
        else:
            data, mime, cache, status = b'Unavailable', 'text/plain', 'no-store', 404
        self.send_response(status)
        self.send_header('Content-Type', mime)
        self.send_header('Cache-Control', cache)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_):
        pass


if __name__ == '__main__':
    print('Client update fixture: http://127.0.0.1:8799', flush=True)
    ThreadingHTTPServer(('127.0.0.1', 8799), Handler).serve_forever()
