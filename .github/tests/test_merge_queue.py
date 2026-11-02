"""Offline contract for the required merge-group CI jobs. No API calls or credentials."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class WorkflowContractTests(unittest.TestCase):
    def test_ci_keeps_pr_and_queue_triggers_and_independent_groups(self):
        ci = (ROOT / '.github/workflows/ci.yml').read_text()
        self.assertIn('  pull_request:', ci)
        self.assertIn('  merge_group:', ci)
        self.assertIn('types: [checks_requested]', ci)
        self.assertIn('github.event.pull_request.number || github.ref', ci)
        self.assertIn('github.event_name', ci)
        self.assertIn('  test:', ci)
        self.assertIn('  workflows:', ci)
        self.assertIn('python3 -m unittest discover -s .github/tests -v', ci)
        self.assertNotIn('  pull_request_target:', ci)
        self.assertIn('  contents: read', ci)
        self.assertNotIn(': write', ci)

    def test_no_app_or_automatic_enrollment_workflow_is_shipped(self):
        self.assertFalse((ROOT / '.github/workflows/auto-merge.yml').exists())
        self.assertFalse((ROOT / '.github/scripts/auto_merge.py').exists())


if __name__ == '__main__':
    unittest.main()
