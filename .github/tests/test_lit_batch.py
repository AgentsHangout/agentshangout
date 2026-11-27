import importlib.util
from pathlib import Path
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/lit_batch.py"
spec = importlib.util.spec_from_file_location("lit_batch", SCRIPT)
batch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(batch)


class LitBatchTests(unittest.TestCase):
    def test_selection_is_opt_in_and_always_includes_requesting_pr(self):
        self.assertEqual(batch.requested_prs("Mention Lit-with: in prose", 7), [])
        self.assertEqual(batch.requested_prs("title\r\nLit-with: 9 8 9 7\r\n", 7), [7, 9, 8])
        for bad in ("7", "", "-1", "--label ready", "8; echo", "8\nLit-with: 9"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                batch.requested_prs("Lit-with: " + bad, 7)

    def test_collected_revisions_must_match_the_event_and_live_same_repo_heads(self):
        repo = "owner/project"
        event = {"repository": {"full_name": repo}, "number": 7,
                 "pull_request": {"body": "Lit-with: 8", "head": {"sha": "a"}}}
        snapshot = {"changes": [{"number": 7, "head": "a"}, {"number": 8, "head": "b"}]}
        def pr(number):
            return {"head": {"repo": {"full_name": repo}, "sha": {7: "a", 8: "b"}[number]},
                    "base": {"ref": "main"}, "state": "open", "draft": False}
        batch.check_sources(snapshot, event, pr)
        for change in ({"head": {"repo": {"full_name": repo}, "sha": "moved"}},
                       {"head": {"repo": {"full_name": "fork/project"}, "sha": "b"}},
                       {"head": {"repo": None, "sha": "b"}}, {"draft": True},
                       {"state": "closed"}, {"base": {"ref": "release"}}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                batch.check_sources(snapshot, event, lambda n: pr(n) | (change if n == 8 else {}))
        for changed in ([{"number": 7, "head": "new"}, {"number": 8, "head": "b"}],
                        [{"number": 7, "head": "a"}]):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                batch.check_sources({"changes": changed}, event, pr)
