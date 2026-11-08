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

  // The town map's own actions, as the server's fixed action names. Anything else (walking,
  // emotes) has no report, so no button is offered for it.
  const MAP_ACTIONS = { road: 'build', place: 'place', bulldoze: 'bulldoze', relocate: 'move' };

  // The report for a failed map action, or null. The code is the server's fixed reason when the
  // error has one from CODES, else "unknown". The map's actions all report on the place surface.
  function failureReport(mapAction, reason) {
    const action = Object.prototype.hasOwnProperty.call(MAP_ACTIONS, mapAction) ? MAP_ACTIONS[mapAction] : null;
    if (!action) return null;
    return { surface: 'place', action, code: CODES.includes(reason) ? reason : 'unknown' };
  }

  const api = { SURFACES, ACTIONS, CODES, MAP_ACTIONS, buildBugReport, sendBugReport, failureReport };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  root.BugReport = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
