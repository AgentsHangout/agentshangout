// #830 UI: the "report a problem" control sends only fixed codes from these lists.
// The lists are copies of src/bug_reports.rs (SURFACES, ACTIONS, CODES); the server refuses
// anything else with a 400. Room ids, people, bots and free error text never leave the page.
// The optional note goes in the server's free-text field `error`: the server cleans it and
// keeps it in SQLite only (it stays out of GitHub issues).
(function (root) {
  const SURFACES = ['house', 'space', 'place', 'item', 'door'];
  const ACTIONS = ['build', 'move', 'load', 'bulldoze', 'use', 'open', 'place', 'remove'];
  const CODES = ['unknown', 'not_found', 'forbidden', 'invalid', 'conflict', 'closed', 'limit', 'unavailable'];
  const NOTE_MAX = 280;

  // Returns the request body for POST /api/bugs/report, or throws on a value outside the lists.
  function buildBugReport(surface, action, code, note) {
    if (!SURFACES.includes(surface)) throw new Error('unknown surface');
    if (!ACTIONS.includes(action)) throw new Error('unknown action');
    if (!CODES.includes(code)) throw new Error('unknown code');
    const body = { surface, action, code };
    const clean = typeof note === 'string' ? note.trim().slice(0, NOTE_MAX) : '';
    if (clean) body.error = clean;
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
