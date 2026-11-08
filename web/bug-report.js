// #830 UI: the "report a problem" control sends only fixed codes from these lists.
// Room ids, people, bots and free error text never leave the page. The optional note is
// capped and goes to SQLite only (the server keeps it out of GitHub issues).
(function (root) {
  const SURFACES = ['house', 'space', 'place', 'item', 'door'];
  const ACTIONS = ['build', 'move', 'use', 'open', 'dream', 'save'];
  const CODES = ['failed', 'timeout', 'forbidden', 'not_found', 'invalid', 'server_error', 'other'];
  const NOTE_MAX = 280;

  // Returns the request body for POST /api/bugs/report, or throws on a value outside the lists.
  function buildBugReport(surface, action, code, note) {
    if (!SURFACES.includes(surface)) throw new Error('unknown surface');
    if (!ACTIONS.includes(action)) throw new Error('unknown action');
    if (!CODES.includes(code)) throw new Error('unknown code');
    const body = { surface, action, error: code };
    const clean = typeof note === 'string' ? note.trim().slice(0, NOTE_MAX) : '';
    if (clean) body.note = clean;
    return body;
  }

  // Sends the report with the page's own session (cookies), like the other app calls.
  async function sendBugReport(api, surface, action, code, note) {
    const body = buildBugReport(surface, action, code, note);
    return api('/api/bugs/report', { method: 'POST', body });
  }

  const api = { SURFACES, ACTIONS, CODES, NOTE_MAX, buildBugReport, sendBugReport };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  root.BugReport = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
