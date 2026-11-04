// Runtime tests for MCP Apps collaboration board (MA7 & MA8).
// Run with: node tests/mcp_apps_ui.cjs
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');

const html = fs.readFileSync('web/collaboration_board.html', 'utf8').replace(/\r\n/g, '\n');
const scriptMatch = html.match(/<script>([\s\S]*?)<\/script>/);
assert.ok(scriptMatch, 'script tag found in collaboration_board.html');
const scriptCode = scriptMatch[1];

class MockElement {
  constructor(id = '', tag = 'div') {
    this.id = id;
    this.tag = tag;
    this.textContent = '';
    this._innerHTML = '';
    this.style = {};
    this.attrs = {};
    this.listeners = {};
    this.disabled = false;
    this.value = '';
    this.children = [];
  }

  get innerHTML() {
    return this._innerHTML;
  }

  set innerHTML(val) {
    this._innerHTML = val;
    // When cards are rendered, create mock card items so click listeners work
    if (val.includes('class="card-item"')) {
      const matches = [...val.matchAll(/class="card-item"\s+data-idx="(\d+)"/g)];
      this.children = matches.map(m => {
        const item = new MockElement('', 'div');
        item.setAttribute('data-idx', m[1]);
        return item;
      });
    } else {
      this.children = [];
    }
  }

  setAttribute(k, v) {
    this.attrs[k] = String(v);
  }

  getAttribute(k) {
    return this.attrs[k] || null;
  }

  removeAttribute(k) {
    delete this.attrs[k];
  }

  addEventListener(event, fn) {
    (this.listeners[event] ||= []).push(fn);
  }

  click() {
    (this.listeners['click'] || []).forEach(fn => fn({ type: 'click', preventDefault() {} }));
  }

  focus() {}

  querySelectorAll(sel) {
    if (sel === '.card-item') {
      return this.children;
    }
    return [];
  }
}

function createEnvironment() {
  const elements = new Map();
  function getEl(id) {
    if (!elements.has(id)) {
      elements.set(id, new MockElement(id));
    }
    return elements.get(id);
  }

  // Pre-seed known elements
  const filterTabs = [
    new MockElement('tab-all'),
    new MockElement('tab-open'),
    new MockElement('tab-closed')
  ];
  filterTabs[0].setAttribute('data-status', 'all');
  filterTabs[1].setAttribute('data-status', 'open');
  filterTabs[2].setAttribute('data-status', 'closed');

  const doc = {
    getElementById(id) {
      return getEl(id);
    },
    querySelectorAll(sel) {
      if (sel === '.filter-tab') return filterTabs;
      return [];
    }
  };

  const sentMessages = [];
  const parent = {
    postMessage(data, origin) {
      sentMessages.push({ data, origin });
    }
  };

  const windowListeners = {};
  const win = {
    parent: parent,
    addEventListener(event, fn) {
      (windowListeners[event] ||= []).push(fn);
    },
    dispatchMessage(msg, source = parent) {
      const handlers = windowListeners['message'] || [];
      handlers.forEach(fn => fn({ data: msg, source: source }));
    }
  };

  const context = {
    window: win,
    document: doc,
    console: console,
    setTimeout: (fn, ms) => setTimeout(fn, Math.min(ms, 30)),
    clearTimeout: clearTimeout,
    Date: Date,
    JSON: JSON,
    parseInt: parseInt,
    encodeURIComponent: encodeURIComponent,
    Math: Math
  };

  vm.createContext(context);
  return { context, elements, sentMessages, win, parent, filterTabs };
}

async function runTests() {
  // Test 1: Lifecycle handshake and protocol negotiation (MA7)
  {
    const env = createEnvironment();
    vm.runInContext(scriptCode, env.context);

    // Verify initial ui/initialize sent to parent
    assert.equal(env.sentMessages.length, 1, 'Initial handshake message sent');
    const initMsg = env.sentMessages[0].data;
    assert.equal(initMsg.jsonrpc, '2.0');
    assert.equal(initMsg.id, 1);
    assert.equal(initMsg.method, 'ui/initialize');
    assert.equal(initMsg.params.protocolVersion, '2026-01-26');
    assert.equal(initMsg.params.appInfo.name, 'Agents Hangout Collaboration Board');
    assert.equal(Object.keys(initMsg.params.appCapabilities).length, 0);

    // Host responds with board data
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: 1,
      result: {
        hangout: { id: 'room-fairground', name: 'Commons Fairground', purpose: 'Build together' },
        whiteboard: { text: 'Building MCP Apps' },
        boards: [
          {
            name: 'Sprint Board',
            kind: 'issues',
            github: 'AgentsHangout/agentshangout',
            cards: [
              { id: 'c1', title: 'Task 1', text: 'Card details here', status: 'open', github_issue: 265 }
            ]
          }
        ],
        people: ['Justin'],
        bots: ['AntiGravity Miner']
      }
    });

    // Verify ui/notifications/initialized was emitted
    const notif = env.sentMessages.find(m => m.data.method === 'ui/notifications/initialized');
    assert.ok(notif, 'ui/notifications/initialized emitted after handshake response');

    // Verify DOM rendered
    assert.equal(env.elements.get('room-name').textContent, 'Commons Fairground');
    assert.equal(env.elements.get('overview-whiteboard').textContent, 'Building MCP Apps');
    assert.ok(env.elements.get('cards-container').innerHTML.includes('Task 1'));
  }

  // Test 2: Selected card conversation handoff with denial handling and feedback (MA8)
  {
    const env = createEnvironment();
    vm.runInContext(scriptCode, env.context);

    // Supply initial data
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: 1,
      result: {
        hangout: { id: 'room-fairground', name: 'Commons Fairground' },
        boards: [{
          name: 'Sprint Board',
          cards: [{ id: 'c1', title: 'Task 1', text: 'Issue detail', status: 'open' }]
        }]
      }
    });

    // User clicks the card to open modal
    const cardsContainer = env.elements.get('cards-container');
    assert.equal(cardsContainer.children.length, 1);
    cardsContainer.children[0].click();

    assert.equal(env.elements.get('modal-card-title').textContent, 'Task 1');
    assert.equal(env.elements.get('card-modal').style.display, 'flex');

    // Clear sent messages to observe handoff
    env.sentMessages.length = 0;

    // User clicks "Send to Chat" (handoff-btn)
    const handoffBtn = env.elements.get('handoff-btn');
    handoffBtn.click();

    // Verify sending state
    assert.equal(handoffBtn.disabled, true);
    assert.equal(handoffBtn.textContent, 'Sending...');

    // Verify ui/message sent with request ID
    assert.equal(env.sentMessages.length, 1, 'ui/message dispatched');
    const msgCall = env.sentMessages[0].data;
    assert.equal(msgCall.jsonrpc, '2.0');
    assert.equal(msgCall.method, 'ui/message');
    assert.ok(typeof msgCall.id === 'number', 'ui/message has numeric ID');
    assert.equal(msgCall.params.role, 'user');
    assert.ok(msgCall.params.content[0].text.includes('Task 1'));

    // Host responds with error/denial: result.isError=true or error object
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: msgCall.id,
      error: { code: -32000, message: 'User denied injection' }
    });

    // VERIFY: Modal does NOT close silently on denial!
    assert.equal(env.elements.get('card-modal').style.display, 'flex', 'Modal stays open on error');
    assert.equal(handoffBtn.disabled, false, 'Button re-enabled on error');
    assert.equal(handoffBtn.textContent, 'Send to Chat');
    const modalFeedback = env.elements.get('modal-feedback');
    assert.ok(modalFeedback.textContent.includes('User denied injection'), 'Feedback shown to user on denial');

    // Wait to ensure NO fallback to ui/sendInput occurs
    await new Promise(r => setTimeout(r, 100));
    const sendInputCalls = env.sentMessages.filter(m => m.data.method === 'ui/sendInput');
    assert.equal(sendInputCalls.length, 0, 'Must NOT fallback or retry with ui/sendInput on denial');

    // Now test successful retry after user re-clicks Send to Chat
    handoffBtn.click();
    assert.equal(env.sentMessages.length, 2);
    const retryCall = env.sentMessages[1].data;

    // Host responds with success (result object without isError)
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: retryCall.id,
      result: { delivered: true }
    });

    // VERIFY: Modal closes on successful delivery
    assert.equal(env.elements.get('card-modal').style.display, 'none', 'Modal closes on success');
  }

  // Test 3: Dialog session race prevention (Card A late success does NOT close Card B dialog or re-enable its button) (MA8)
  {
    const env = createEnvironment();
    vm.runInContext(scriptCode, env.context);

    // Initial setup with two cards: Card A and Card B
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: 1,
      result: {
        hangout: { id: 'room-fairground', name: 'Race Test Room' },
        boards: [{
          name: 'Sprint Board',
          cards: [
            { id: 'c1', title: 'Card A', text: 'Detail A', status: 'open' },
            { id: 'c2', title: 'Card B', text: 'Detail B', status: 'open' }
          ]
        }]
      }
    });

    const cardsContainer = env.elements.get('cards-container');
    assert.equal(cardsContainer.children.length, 2);

    // 1. Open Card A dialog and send handoff
    cardsContainer.children[0].click();
    assert.equal(env.elements.get('modal-card-title').textContent, 'Card A');
    const handoffBtn = env.elements.get('handoff-btn');
    handoffBtn.click();
    assert.equal(handoffBtn.disabled, true);
    assert.equal(handoffBtn.textContent, 'Sending...');

    const reqA = env.sentMessages.find(m => m.data.method === 'ui/message');
    const reqIdA = reqA.data.id;

    // 2. User closes Card A's dialog
    env.elements.get('modal-close-btn').click();
    assert.equal(env.elements.get('card-modal').style.display, 'none');

    // 3. User opens Card B's dialog and sends handoff
    cardsContainer.children[1].click();
    assert.equal(env.elements.get('modal-card-title').textContent, 'Card B');
    assert.equal(env.elements.get('card-modal').style.display, 'flex');
    handoffBtn.click();
    assert.equal(handoffBtn.disabled, true);
    assert.equal(handoffBtn.textContent, 'Sending...');

    const reqB = env.sentMessages.filter(m => m.data.method === 'ui/message')[1];
    const reqIdB = reqB.data.id;
    assert.notEqual(reqIdA, reqIdB);

    // 4. Late success for Card A arrives while Card B is still pending!
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: reqIdA,
      result: { delivered: true }
    });

    // VERIFY: Card B's dialog MUST NOT be closed, and Card B's button MUST remain 'Sending...' and disabled!
    assert.equal(env.elements.get('card-modal').style.display, 'flex', 'Card B dialog stays open despite late Card A response');
    assert.equal(env.elements.get('modal-card-title').textContent, 'Card B');
    assert.equal(handoffBtn.disabled, true, 'Card B button remains disabled while B is pending');
    assert.equal(handoffBtn.textContent, 'Sending...', 'Card B button remains in Sending state');

    // 5. Finally Card B's own response arrives
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: reqIdB,
      result: { delivered: true }
    });

    // VERIFY: Now Card B's dialog closes cleanly
    assert.equal(env.elements.get('card-modal').style.display, 'none', 'Card B dialog closes upon its own response');
  }

  // Test 4: Refresh request correlation & rejection of unrelated/late responses (MA7)
  {
    const env = createEnvironment();
    vm.runInContext(scriptCode, env.context);

    // Initial setup
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: 1,
      result: {
        hangout: { id: 'room-fairground', name: 'Original Room' },
        boards: [{ name: 'B1', cards: [{ title: 'Original Card', status: 'open' }] }]
      }
    });

    assert.equal(env.elements.get('room-name').textContent, 'Original Room');

    // Click Refresh
    env.sentMessages.length = 0;
    const refreshBtn = env.elements.get('refresh-btn');
    refreshBtn.click();

    assert.equal(refreshBtn.disabled, true);
    assert.equal(refreshBtn.textContent, 'Refreshing...');

    // Verify tools/call sent with new request id
    assert.equal(env.sentMessages.length, 1);
    const refreshCall = env.sentMessages[0].data;
    assert.equal(refreshCall.method, 'tools/call');
    assert.equal(refreshCall.params.name, 'open_collaboration_board');
    assert.equal(refreshCall.params.arguments.hangout_id, 'room-fairground');
    const refreshReqId = refreshCall.id;
    assert.ok(typeof refreshReqId === 'number');

    // Simulate an UNRELATED or late host response with a DIFFERENT id
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: 999999, // Unrelated ID
      result: {
        hangout: { id: 'spoofed-room', name: 'Spoofed Room Name' },
        boards: [{ name: 'Spoofed Board', cards: [{ title: 'Spoofed Card' }] }]
      }
    });

    // VERIFY: The board did NOT accept the unrelated response!
    assert.equal(env.elements.get('room-name').textContent, 'Original Room', 'Unrelated response ignored');
    assert.ok(env.elements.get('cards-container').innerHTML.includes('Original Card'));

    // Now send the CORRELATED response with the matching refreshReqId
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: refreshReqId,
      result: {
        hangout: { id: 'room-fairground', name: 'Refreshed Room Name' },
        boards: [{ name: 'B1', cards: [{ title: 'Refreshed Card', status: 'open' }] }]
      }
    });

    // VERIFY: Correlated response updated the board and re-enabled the button!
    assert.equal(env.elements.get('room-name').textContent, 'Refreshed Room Name', 'Correlated response accepted');
    assert.ok(env.elements.get('cards-container').innerHTML.includes('Refreshed Card'));
    assert.equal(refreshBtn.disabled, false);
    assert.equal(refreshBtn.textContent, 'Refresh');
  }

  // Test 5: Refresh error handling with result.isError=true and stale data feedback (MA7)
  {
    const env = createEnvironment();
    vm.runInContext(scriptCode, env.context);

    // Initial setup
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: 1,
      result: {
        hangout: { id: 'room-fairground', name: 'Existing Room' },
        boards: [{ name: 'B1', cards: [{ title: 'Existing Card', status: 'open' }] }]
      }
    });

    // Click Refresh
    env.sentMessages.length = 0;
    const refreshBtn = env.elements.get('refresh-btn');
    refreshBtn.click();
    assert.equal(refreshBtn.disabled, true);

    const refreshCall = env.sentMessages[0].data;

    // Host responds with tool failure (result.isError = true)
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: refreshCall.id,
      result: {
        content: [{ type: 'text', text: 'Room connection lost' }],
        isError: true
      }
    });

    // VERIFY: Refresh button is re-enabled, stale data remains, and stale-notice shows error feedback!
    assert.equal(refreshBtn.disabled, false, 'Refresh button re-enabled on isError');
    assert.equal(refreshBtn.textContent, 'Refresh');
    assert.equal(env.elements.get('room-name').textContent, 'Existing Room', 'Stale data kept');
    const staleNotice = env.elements.get('stale-notice');
    assert.ok(staleNotice.textContent.includes('Refresh failed: Room connection lost'), 'Feedback given on isError');
  }

  // Test 6: Unanswered refresh and handoff timeout handling (MA7 & MA8)
  {
    const env = createEnvironment();
    vm.runInContext(scriptCode, env.context);

    // Initial setup
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: 1,
      result: {
        hangout: { id: 'room-fairground', name: 'Existing Room' },
        boards: [{ name: 'B1', cards: [{ title: 'Card 1', text: 'Detail', status: 'open' }] }]
      }
    });

    // 1. Unanswered Refresh:
    const refreshBtn = env.elements.get('refresh-btn');
    refreshBtn.click();
    assert.equal(refreshBtn.disabled, true);

    // Wait for mock timer
    await new Promise(r => setTimeout(r, 60));

    // VERIFY: Refresh button is re-enabled without permanent lock
    assert.equal(refreshBtn.disabled, false, 'Unanswered refresh re-enables button after timeout');
    assert.equal(refreshBtn.textContent, 'Refresh');
    const staleNotice = env.elements.get('stale-notice');
    assert.ok(staleNotice.textContent.includes('Refresh timed out'), 'Stale notice shows timeout');

    // 2. Unanswered Handoff:
    const cardsContainer = env.elements.get('cards-container');
    cardsContainer.children[0].click();
    const handoffBtn = env.elements.get('handoff-btn');
    handoffBtn.click();
    assert.equal(handoffBtn.disabled, true);

    // Wait for mock timer
    await new Promise(r => setTimeout(r, 60));

    // VERIFY: Handoff button is re-enabled and feedback is shown without silent closure or retry
    assert.equal(handoffBtn.disabled, false, 'Unanswered handoff re-enables button after timeout');
    assert.equal(env.elements.get('card-modal').style.display, 'flex', 'Modal stays open on timeout');
    const modalFeedback = env.elements.get('modal-feedback');
    assert.ok(modalFeedback.textContent.includes('No response from host'), 'Feedback shown on timeout');
  }

  // Test 7: Security - ignore messages from untrusted window sources (MA7)
  {
    const env = createEnvironment();
    vm.runInContext(scriptCode, env.context);

    // Dispatch message with foreign source !== window.parent
    const untrustedSource = {};
    env.win.dispatchMessage({
      jsonrpc: '2.0',
      id: 1,
      result: {
        hangout: { name: 'Untrusted Attacker Room' }
      }
    }, untrustedSource);

    // Initial state remained waiting and untrusted data was never rendered
    assert.equal(env.elements.get('room-name'), undefined);
    assert.ok(env.elements.get('cards-container').innerHTML.includes('Waiting for collaboration board data'));
  }

  console.log('All MCP Apps UI runtime tests passed (handshake, handoff, correlation, isError handling, timeout, security).');
}

runTests().catch(err => {
  console.error(err);
  process.exit(1);
});
