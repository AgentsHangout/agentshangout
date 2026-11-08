// #830 UI: the "report a problem" control sends only fixed codes from these lists.
// The lists are copies of src/bug_reports.rs (SURFACES, ACTIONS, CODES); the server refuses
// anything else with a 400. No free text is sent: the note field is left out of the UI for now
// (a free-text note can come later as its own issue, with privacy review).
(function (root) {
  const SURFACES = ['house', 'space', 'place', 'item', 'door'];
  const ACTIONS = ['build', 'move', 'load', 'bulldoze', 'use', 'open', 'place', 'remove'];
  const CODES = ['unknown', 'not_found', 'forbidden', 'invalid', 'conflict', 'closed', 'limit', 'unavailable'];

  // Returns the request body for POST /api/bugs/report, or throws on a value outside the lists.
  function buildBugReport(surface, action, code) {
    if (!SURFACES.includes(surface)) throw new Error('unknown surface');
    if (!ACTIONS.includes(action)) throw new Error('unknown action');
    if (!CODES.includes(code)) throw new Error('unknown code');
    return { surface, action, code };
  }

  // Sends the report with the page's own session (cookies), like the other app calls.
  async function sendBugReport(api, surface, action, code) {
    const body = buildBugReport(surface, action, code);
    return api('/api/bugs/report', { method: 'POST', body });
  }

  // The short code for a failed request: the server's reason when it is on the list, else the
  // HTTP status's meaning, else 'unknown'. The error's own message is never used.
  const STATUS_CODES = { 400: 'invalid', 403: 'forbidden', 404: 'not_found', 409: 'conflict', 410: 'closed', 429: 'limit', 503: 'unavailable' };
  function codeFor(error) {
    if (error && CODES.includes(error.reason)) return error.reason;
    const code = error && STATUS_CODES[error.status];
    return code || 'unknown';
  }

  const api = { SURFACES, ACTIONS, CODES, buildBugReport, sendBugReport, codeFor };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  root.BugReport = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
