"""W115: docked panels reserve real scene space; floating is explicit."""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread
from playwright.sync_api import sync_playwright


class Quiet(SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


def overlaps(a, b):
    return min(a['x']+a['width'], b['x']+b['width']) - max(a['x'], b['x']) > 1 and \
        min(a['y']+a['height'], b['y']+b['height']) - max(a['y'], b['y']) > 1


root = Path(__file__).resolve().parents[1]
server = ThreadingHTTPServer(('127.0.0.1', 0), partial(Quiet, directory=str(root)))
Thread(target=server.serve_forever, daemon=True).start()
with sync_playwright() as pw:
    browser = pw.chromium.launch()
    for width, height in [(1280, 900), (1600, 1100), (393, 852), (852, 393), (320, 568)]:
        context = browser.new_context(viewport={'width': width, 'height': height}, has_touch=width < 1000)
        page = context.new_page()
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        url = f'http://127.0.0.1:{server.server_port}/tests/game_windows.html' + ('?phone' if width < 1000 else '')
        page.goto(url)
        page.wait_for_function('!!window.fixture')
        page.wait_for_timeout(100)

        def clear_scene():
            scene = page.locator('.wd-well').bounding_box()
            assert scene and scene['width'] > 100 and scene['height'] > 80, scene
            stage = page.locator('.stage').bounding_box()
            assert stage['x'] >= scene['x']-1 and stage['y'] >= scene['y']-1
            assert stage['x']+stage['width'] <= scene['x']+scene['width']+1
            assert stage['y']+stage['height'] <= scene['y']+scene['height']+1, 'room remains inside reserved scene'
            windows = page.locator('.game-window:visible')
            boxes = [windows.nth(i).bounding_box() for i in range(windows.count())]
            for i, box in enumerate(boxes):
                assert not overlaps(scene, box), f'docked window overlaps scene: {scene}, {box}'
                assert all(not overlaps(box, other) for other in boxes[i+1:]), 'windows overlap'

        clear_scene()  # Reproduces the original overlay bug before W115.
        # Dragging across the room never silently undocks; snapping previews before commit.
        head = page.locator('#panel-bots .game-panel-head')
        head.evaluate("el => el.addEventListener('pointerdown', e => window.dragPointer = e.pointerId)")
        start = head.bounding_box()
        scene = page.locator('.wd-well').bounding_box()
        page.mouse.move(start['x']+12, start['y']+start['height']/2)
        page.mouse.down()
        page.mouse.move(scene['x']+scene['width']/2, scene['y']+scene['height']/2, steps=4)
        clear_scene()
        assert page.locator('#panel-bots').get_attribute('data-zone') == 'right'
        workspace = page.locator('.window-workspace').bounding_box()
        page.mouse.move(width/2, workspace['y']+10, steps=4)
        assert page.locator('.window-snap-preview').is_visible()
        head.evaluate("el => el.dispatchEvent(new PointerEvent('pointercancel', {pointerId: window.dragPointer, bubbles:true}))")
        page.mouse.up()
        assert page.locator('.window-snap-preview').is_hidden()
        assert page.locator('#panel-bots').get_attribute('data-zone') == 'right'
        start = head.bounding_box()
        page.mouse.move(start['x']+12, start['y']+start['height']/2)
        page.mouse.down()
        page.mouse.move(width/2, workspace['y']+10, steps=4)
        page.mouse.up()
        assert page.locator('#panel-bots').get_attribute('data-zone') == 'top'
        clear_scene()
        page.evaluate("fixture.panels.dock('room', 'right'); fixture.panels.dock('bots', 'right')")
        clear_scene()
        page.get_by_role('tab', name='Chat', exact=True).click()
        assert page.locator('#panel-room').is_visible() and page.locator('#panel-bots').is_hidden()
        assert page.get_by_role('textbox', name='Message').input_value() == 'Keep this draft'
        page.get_by_role('tab', name='My bots', exact=True).click()
        page.locator('#panel-bots').get_by_role('button', name='Layout My bots', exact=True).click()
        page.get_by_label('Position for My bots', exact=True).select_option('left')
        clear_scene()
        page.evaluate("fixture.panels.preset('split')")
        clear_scene()
        page.evaluate("fixture.panels.preset('bottom')")
        clear_scene()
        divider = page.locator('.window-zone-divider:visible').first
        old_scene = page.locator('.wd-well').bounding_box()
        divider.focus()
        page.keyboard.press('ArrowLeft' if divider.get_attribute('aria-orientation') == 'vertical' else 'ArrowUp')
        assert page.locator('.wd-well').bounding_box() != old_scene, 'boundary resizes the actual scene'
        clear_scene()
        if width < 1000:
            # A real touch drag resizes the shared scene boundary, including landscape.
            boundary = divider.bounding_box()
            x, y = boundary['x']+boundary['width']/2, boundary['y']+boundary['height']/2
            vertical = divider.get_attribute('aria-orientation') == 'vertical'
            before_touch = page.locator('.wd-well').bounding_box()
            cdp = context.new_cdp_session(page)
            cdp.send('Input.dispatchTouchEvent', {'type':'touchStart', 'touchPoints':[{'x':x, 'y':y}]})
            cdp.send('Input.dispatchTouchEvent', {'type':'touchMove', 'touchPoints':[{'x':x-20 if vertical else x, 'y':y if vertical else y-20}]})
            cdp.send('Input.dispatchTouchEvent', {'type':'touchEnd', 'touchPoints':[]})
            cdp.detach()
            assert page.locator('.wd-well').bounding_box() != before_touch, 'touch resizes scene boundary'
            clear_scene()
        page.locator('#panel-bots').get_by_role('button', name='Layout My bots', exact=True).click()
        page.get_by_label('Position for My bots', exact=True).select_option('float')
        assert page.locator('#panel-bots').get_attribute('data-zone') == 'float'
        page.evaluate("fixture.panels.dock('bots', 'left')")
        page.reload()
        page.wait_for_function('!!window.fixture')
        assert page.locator('#panel-bots').get_attribute('data-zone') == 'left', 'saved preference survives reload'
        clear_scene()
        page.evaluate('fixture.panels.resetLayout()')
        clear_scene()
        if width >= 1600:
            page.evaluate("""() => {
              fixture.panels.add('notes', 'Notes', {}, fixture.h('p', {}, 'Room notes'));
              fixture.panels.add('plans', 'Plans', {}, fixture.h('p', {}, 'Room plans'));
              fixture.panels.open('notes'); fixture.panels.open('plans');
              fixture.panels.dock('room', 'left'); fixture.panels.dock('bots', 'right');
              fixture.panels.dock('notes', 'top'); fixture.panels.dock('plans', 'bottom');
            }""")
            assert page.locator('.game-window:visible').count() == 4
            clear_scene()
            page.set_viewport_size({'width':393, 'height':852})
            page.wait_for_timeout(100)
            assert page.locator('.game-window:visible').count() == 1, 'small screens consolidate zones'
            clear_scene()
            page.set_viewport_size({'width':width, 'height':height})
            page.wait_for_timeout(100)
            assert page.locator('.game-window:visible').count() == 4, 'rotation restores zone preferences'
            clear_scene()
        page.evaluate("fixture.panels.maximize('bots')")
        assert not page.locator('.wd-well').is_visible()
        page.evaluate("fixture.panels.maximize('bots')")
        clear_scene()
        page.evaluate('fixture.panels.minimizeAll()')
        assert page.locator('.window-dock').is_visible()
        assert not overlaps(page.locator('.wd-well').bounding_box(), page.locator('.window-dock').bounding_box())
        page.get_by_role('button', name='Restore Chat', exact=True).click()
        clear_scene()
        assert not errors, errors
        print(f'PASS W115 {width}x{height}')
        context.close()
    context = browser.new_context(viewport={'width': 1000, 'height': 800})
    context.add_init_script("Object.defineProperty(window, 'localStorage', {get(){throw new Error('storage blocked')}})")
    page = context.new_page()
    page.goto(f'http://127.0.0.1:{server.server_port}/tests/game_windows.html')
    page.wait_for_function('!!window.fixture')
    page.evaluate("fixture.panels.dock('bots', 'left'); fixture.panels.resetLayout()")
    assert page.locator('#panel-bots').is_visible(), 'blocked storage still permits layout changes'
    context.close()
    browser.close()
server.shutdown()
