"""Real-browser A/B update checks. Start client_update_server.py first. Requires Playwright."""
from playwright.sync_api import sync_playwright

BASE = 'http://127.0.0.1:8799'

with sync_playwright() as p:
    browser = p.chromium.launch(headless=True)
    context = browser.new_context(service_workers='allow')
    context.set_default_timeout(15000)
    page = context.new_page()
    page.goto(BASE + '/__revision?value=A')
    page.goto(BASE)
    page.wait_for_function("document.querySelector('#revision').textContent === 'A'")
    page.evaluate('() => navigator.serviceWorker.ready')
    page.wait_for_function('!!navigator.serviceWorker.controller')
    page.fill('#draft', 'keep this unsent message')
    second = context.new_page()
    second.goto(BASE)
    second.wait_for_function("document.querySelector('#revision').textContent === 'A'")
    second.fill('#draft', 'other tab draft')
    page.evaluate("() => fetch('/__revision?value=B')")
    page.evaluate('async () => (await navigator.serviceWorker.getRegistration()).update()')
    page.wait_for_function("document.querySelector('#status').textContent === 'ready'")
    assert page.text_content('#revision') == 'A'
    assert page.input_value('#draft') == 'keep this unsent message'
    page.once('dialog', lambda d: d.accept())
    page.click('#refresh')
    page.wait_for_function("document.querySelector('#revision').textContent === 'B'")
    assert second.text_content('#revision') == 'A'
    assert second.input_value('#draft') == 'other tab draft'
    context.set_offline(True)
    assert second.evaluate("async () => { const r = await fetch(document.querySelector('script').src.replace('client-entry.js', 'app.js')); return r.ok && (await r.text()).includes(\"= 'A'\"); }")
    context.set_offline(False)
    page.reload()
    page.wait_for_function("document.querySelector('#revision').textContent === 'B'")
    print('PASS: A-to-B update, waiting update preserves both drafts, explicit refresh updates one tab, old assets remain usable offline, warm reload uses B.')
    context.close()
    context = browser.new_context(service_workers='allow')
    page = context.new_page()
    page.route('**/client/**/app.js', lambda route: route.abort())
    page.goto(BASE)
    page.get_by_role('heading', name="The app couldn't finish loading").wait_for()
    assert page.get_by_role('button', name='Reload app').is_visible()
    print('PASS: interrupted cold module load shows an explicit reload screen.')
    context.close()
    browser.close()
