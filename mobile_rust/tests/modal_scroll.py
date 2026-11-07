"""M11: exercise production phone dialogs and sheets in real Chromium."""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread
from playwright.sync_api import sync_playwright


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


root = Path(__file__).resolve().parents[2]
server = ThreadingHTTPServer(("127.0.0.1", 0), partial(QuietHandler, directory=str(root)))
Thread(target=server.serve_forever, daemon=True).start()
failures = []
with sync_playwright() as playwright:
    browser = playwright.chromium.launch(headless=True)
    for width, height in [(393, 852), (852, 393), (320, 568)]:
        context = browser.new_context(viewport={"width": width, "height": height}, has_touch=True)
        page = context.new_page()
        page.goto(f"http://127.0.0.1:{server.server_port}/")
        page.set_content('<link rel="stylesheet" href="/web/style.css">')
        page.evaluate(r"""async () => {
          const source = (await (await fetch('/web/app.js')).text()).replace(/\r\n/g, '\n');
          const cut = name => {
            const start = source.indexOf(`function ${name}(`);
            return source.slice(start, source.indexOf('\n}\n', start) + 3);
          };
          const names = ['h','modal','formError','field','submitter','stayField','schedulePainter',
            'routineDialog','installSheetDrag','createPanels','watchPhoneChrome','installAppleHigMap'];
          const code = names.map(name => `${name === 'routineDialog' ? 'async ' : ''}${cut(name)}`).join('\n');
          // Mock only API data; use the actual schedule controls, modal and sheet builders.
          const setup = `const api=async()=>({days:[],starts:'09:00',ends:'17:00',workplace_choices:[],public_places:false});
            const STRANGERS_TEXT='Public room warning';`;
          const {h, modal, routineDialog, createPanels, installAppleHigMap} = new Function(
            `${setup}\n${code}\nreturn {h,modal,routineDialog,createPanels,installAppleHigMap};`)();
          const {windowFrame} = await import('/web/city.js');
          const frame = windowFrame({label:'Test town',stats:[],tools:true});
          frame.root.classList.add('wd-game','apple-hig'); document.body.append(frame.root);
          for(let i=0;i<30;i++) frame.tools.append(h('button',{class:'wd-btn'},`Building ${i}`));
          const panels = createPanels(frame.root);
          for(const name of ['more','bots','around']) panels.add(name, name, {},
            h('ul',{class:name === 'around' ? 'around' : 'stack'},
              ...Array.from({length:50},(_,i)=>h('li',{},h('button',{},`${name} entry ${i}`)))));
          const build = installAppleHigMap({frame,panels,choose:()=>{},movesCount:()=>300,trust:true});
          window.fixture={panels,build,whiteboard:()=>modal('Whiteboard',
            h('div',{class:'board-read'},Array.from({length:100},(_,i)=>`Note ${i}\n`).join('')))};
          await routineDialog({id:'bot',name:'Scout'});
        }""")

        def check(condition, message):
            if not condition:
                failures.append(f'{width}x{height}: {message}')

        def visible_inside(control, container):
            box, bounds = control.bounding_box(), container.bounding_box()
            return box and bounds and box['y'] >= max(0, bounds['y']) - 1 and \
                box['y'] + box['height'] <= min(height, bounds['y'] + bounds['height']) + 1

        check(page.evaluate('matchMedia("(pointer: coarse)").matches'), 'touch pointer activates mobile styles')

        for kind in ['schedule', 'whiteboard']:
            if kind == 'whiteboard':
                page.evaluate('fixture.whiteboard()')
            modal = page.locator('dialog.modal')
            if kind == 'whiteboard':
                check(not modal.locator('.board-read').evaluate('(el)=>el.scrollHeight > el.clientHeight+1'),
                      'whiteboard creates nested vertical scrolling')
            modal.evaluate('(el)=>{el.scrollTop=el.scrollHeight}')
            close = modal.locator('.modal-head button')
            reachable = visible_inside(close, modal)
            check(reachable, f'{kind} Close remains visible after scrolling')
            if reachable:
                close.click()
            else:
                modal.evaluate('(el)=>el.close()')
            page.wait_for_function('!document.querySelector("dialog.modal")')

        page.evaluate('fixture.build.openBuildSheet()')
        sheet = page.locator('.apple-build-sheet')
        page.wait_for_timeout(400)
        page.evaluate('document.querySelectorAll(".apple-build-sheet, .apple-tools-grid").forEach(el=>el.scrollTop=el.scrollHeight)')
        close = sheet.locator('.sheet-close')
        check(sheet.evaluate('(el)=>el.scrollHeight <= el.clientHeight+1'), 'Build shell is not a second scroller')
        check(sheet.locator('.apple-tools-grid').evaluate('(el)=>el.scrollTop>0'), 'Build tools scroll to their end')
        check(close.count() == 1 and visible_inside(close, sheet), 'Build has a persistent visible Close button')
        check(visible_inside(sheet.locator('.sheet-grabber'), sheet), 'Build dismiss handle stays visible')
        if close.count() and visible_inside(close, sheet):
            close.click()
            check(not sheet.evaluate('(el)=>el.classList.contains("open")'), 'Build Close dismisses sheet')
        page.evaluate('fixture.build.closeBuildSheet()')

        for name in ['more','bots','around']:
            page.evaluate('(name)=>fixture.panels.open(name)', name)
            panel = page.locator(f'.game-panel-{name}')
            page.wait_for_timeout(350)
            body = panel.locator('.game-panel-body')
            body.evaluate('(el)=>{el.scrollTop=el.scrollHeight}')
            check(visible_inside(panel.locator('.sheet-close'), panel), f'{name} Close stays visible')
            check(panel.evaluate('(el)=>el.scrollHeight <= el.clientHeight+1'), f'{name} panel is not a second scroller')
            check(body.evaluate('(el)=>el.scrollTop>0'), f'{name} content can scroll to its end')
            check(not body.locator('ul').evaluate('(el)=>el.scrollHeight > el.clientHeight+1'), f'{name} list uses body scroller')
            box = panel.bounding_box()
            check(box['x'] >= 0 and box['x']+box['width'] <= width+1, f'{name} fits viewport width')
            if width > height:
                rail = page.locator('.apple-tab-bar').bounding_box()
                check(box['x'] >= rail['x']+rail['width']-1, f'{name} clears landscape navigation rail')
            panel.locator('.sheet-close').click()
            check(panel.is_hidden(), f'{name} Close dismisses panel')
        context.close()
    browser.close()
server.shutdown()
if failures:
    raise SystemExit('\n'.join(failures))
print('M11 PASS: real Schedule/Whiteboard dialogs and Build/More/My bots/Around sheets, portrait and landscape')
