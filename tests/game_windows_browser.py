"""W114: production window interactions in isolated Chromium, desktop and touch."""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread
from playwright.sync_api import sync_playwright


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


root = Path(__file__).resolve().parents[1]
server = ThreadingHTTPServer(('127.0.0.1', 0), partial(QuietHandler, directory=str(root)))
Thread(target=server.serve_forever, daemon=True).start()
with sync_playwright() as pw:
    browser = pw.chromium.launch(headless=True)
    for width, height, touch in [(1280, 800, False), (393, 852, True), (852, 393, True), (320, 568, True)]:
        context = browser.new_context(viewport={'width': width, 'height': height}, has_touch=touch)
        page = context.new_page()
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto(f'http://127.0.0.1:{server.server_port}/tests/game_windows.html' + ('?phone' if touch else ''))
        page.wait_for_function('!!window.fixture')
        panel = page.locator('#panel-bots')
        assert page.locator('#panel-room').is_visible(), 'second panel keeps chat open'
        before = panel.bounding_box()
        panel.get_by_role('button', name='Maximize My bots', exact=True).click()
        maximized = panel.bounding_box()
        assert maximized['width'] >= before['width'] and maximized['height'] >= before['height']
        panel.get_by_role('button', name='Restore My bots', exact=True).click()
        assert panel.bounding_box() == before, f'restore returns to original geometry: {before} -> {panel.bounding_box()}'
        panel.locator('.game-panel-head').focus()
        page.keyboard.press('ArrowLeft')
        moved = panel.bounding_box()
        assert moved['x'] <= before['x'], 'keyboard title movement'
        panel.locator('.window-resize').focus()
        page.keyboard.press('ArrowLeft')
        assert panel.bounding_box()['width'] < before['width'], 'keyboard resizing'
        # Touch capture uses the same grip as mouse capture, including on a narrow phone.
        grip = panel.locator('.window-resize').bounding_box()
        old_size = panel.bounding_box()
        x, y = grip['x'] + grip['width']/2, grip['y'] + grip['height']/2
        if touch:
            cdp = context.new_cdp_session(page)
            cdp.send('Input.dispatchTouchEvent', {'type': 'touchStart', 'touchPoints': [{'x': x, 'y': y}]})
            cdp.send('Input.dispatchTouchEvent', {'type': 'touchMove', 'touchPoints': [{'x': x-24, 'y': y-24}]})
            cdp.send('Input.dispatchTouchEvent', {'type': 'touchEnd', 'touchPoints': []})
            cdp.detach()
        else:
            page.mouse.move(x, y)
            page.mouse.down()
            page.mouse.move(x-24, y-24, steps=3)
            page.mouse.up()
        assert panel.bounding_box()['height'] < old_size['height'], 'pointer grip resizes window'
        # Real captured pointer drag, clamped to the game well.
        head = panel.locator('.game-panel-head').bounding_box()
        page.mouse.move(head['x'] + 15, head['y'] + head['height']/2)
        page.mouse.down()
        page.mouse.move(-100, -100, steps=5)
        page.mouse.up()
        box = panel.bounding_box()
        assert box['x'] >= 0 and box['y'] >= 0, 'drag cannot lose title off screen'
        panel.get_by_role('button', name='Minimize My bots', exact=True).click()
        assert panel.is_hidden()
        dock = page.locator('.window-dock')
        assert dock.is_visible()
        dock.get_by_role('button', name='Restore My bots', exact=True).click()
        assert panel.is_visible() and dock.is_hidden()
        # Clicking the world keeps the workspace open; its title remains reachable after scroll.
        page.locator('.wd-well').dispatch_event('click')
        assert panel.is_visible()
        body = panel.locator(':scope > .game-panel-body')
        body.evaluate('(el) => {el.scrollTop = el.scrollHeight}')
        assert body.evaluate('(el) => el.scrollTop > 0'), 'outer body scrolls'
        assert page.evaluate('getComputedStyle(fixture.nested).overflowY') == 'visible', 'no nested body scrollbar'
        close = panel.get_by_role('button', name='Close My bots', exact=True)
        assert close.is_visible()
        if touch:
            assert close.bounding_box()['height'] >= 44
            rail = page.locator('.apple-tab-bar').bounding_box()
            if width > height:
                assert box['x'] >= rail['x'] + rail['width'], 'landscape rail is clear'
            else:
                assert box['y'] + box['height'] <= rail['y'], 'bottom navigation is clear'
        close.click()
        chat = page.locator('#panel-room')
        chat.get_by_role('button', name='Minimize Chat', exact=True).click()
        handle = dock.locator('.window-dock-handle')
        original = dock.bounding_box()
        handle.focus()
        page.keyboard.press('ArrowUp')
        assert dock.bounding_box()['y'] < original['y'], 'dock can move away from the bottom'
        dock.get_by_role('button', name='Restore Chat', exact=True).click()
        assert chat.get_by_role('textbox').input_value() == 'Keep this draft'
        page.set_viewport_size({'width': 320, 'height': 568})
        page.wait_for_timeout(100)
        rect = chat.bounding_box()
        assert rect['x'] >= 0 and rect['x'] + rect['width'] <= 321
        assert page.evaluate('document.documentElement.scrollWidth <= innerWidth && document.documentElement.scrollHeight <= innerHeight')
        chat.get_by_role('button', name='Minimize Chat', exact=True).click()
        dock.get_by_role('button', name='Close minimized Chat', exact=True).click()
        assert dock.is_hidden() and chat.is_hidden(), 'X removes a minimized window from the dock'
        assert not errors, errors
        print(f'PASS W114 {width}x{height}, touch={touch}')
        context.close()
    browser.close()
server.shutdown()
