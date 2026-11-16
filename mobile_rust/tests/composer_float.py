#!/usr/bin/env python3
"""M12 (#835): a floating chat window on a phone docks its composer on the soft keyboard.

Opens mobile_rust/tests/composer-float.html in headless Chromium at 393 x 852, serving the repo
from a local thread so no files outside the checkout are needed. The page reports PASS or FAIL in
its title. Needs the playwright package and a chromium install (`playwright install chromium`).
"""
import http.server
import pathlib
import sys
import threading

from playwright.sync_api import sync_playwright

REPO = pathlib.Path(__file__).resolve().parents[2]


class Quiet(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(REPO), **kwargs)

    def log_message(self, *args):
        pass


def main():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Quiet)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_address[1]}/mobile_rust/tests/composer-float.html"
    try:
        with sync_playwright() as p:
            browser = p.chromium.launch()
            page = browser.new_page(viewport={"width": 393, "height": 852}, is_mobile=True, has_touch=True)
            errors = []
            page.on("pageerror", lambda e: errors.append(str(e)))
            page.goto(url, wait_until="load", timeout=60000)
            page.wait_for_function("document.title === 'PASS' || document.title === 'FAIL'", timeout=60000)
            print(page.inner_text("#results"))
            verdict = page.title()
            browser.close()
    finally:
        server.shutdown()
    if errors:
        print("page errors:", errors)
    print(verdict)
    return 0 if verdict == "PASS" and not errors else 1


if __name__ == "__main__":
    sys.exit(main())
