"""The release packager must produce a complete, content-addressed public client."""

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location("client_bundle", Path(__file__).resolve().parents[1] / "tools/client_bundle.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ClientBundleTests(unittest.TestCase):
    def test_complete_archive_and_manifest_are_reproducible(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            web = root / "web"
            web.mkdir()
            for name, text in {"index.html": "<div id=app></div>", "app.js": "import { room } from './room.js';",
                               "room.js": "export const room = 1;", "style.css": "body {}",
                               "home.html": "public website home", "server.local.env": "not a client asset"}.items():
                (web / name).write_text(text)
            first = MODULE.package(web, root / "first", "mobile-assets-test")
            MODULE.package(web, root / "second", "mobile-assets-test")
            archive = root / "first/android-client.zip"
            self.assertEqual(archive.read_bytes(), (root / "second/android-client.zip").read_bytes())
            self.assertEqual(first["archive_sha256"], hashlib.sha256(archive.read_bytes()).hexdigest())
            with zipfile.ZipFile(archive) as bundle:
                self.assertEqual(set(bundle.namelist()), {"index.html", "app.js", "room.js", "style.css"})
                for name in bundle.namelist():
                    self.assertEqual(first["files"][name]["sha256"], hashlib.sha256(bundle.read(name)).hexdigest())
            self.assertEqual(first, json.loads((root / "first/android-client.json").read_text()))

    def test_missing_import_and_unapproved_tag_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, text in {"index.html": "entry", "style.css": "css", "app.js": "import { a } from './missing.js';"}.items():
                (root / name).write_text(text)
            with self.assertRaises(ValueError):
                MODULE.package(root, root / "out", "main")
            with self.assertRaisesRegex(ValueError, "imports an asset outside"):
                MODULE.package(root, root / "out", "mobile-assets-test")


if __name__ == "__main__":
    unittest.main()
