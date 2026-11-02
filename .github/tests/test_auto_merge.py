"""Offline tests: no GitHub writes or tokens. Run with unittest discovery."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / '.github/scripts/auto_merge.py'


class AutoMergeTests(unittest.TestCase):
    def run_case(self, changes=None, env_changes=None, view_exit=0, merge_exit=0, rules=None):
        if rules is None:
            rules = [dict(type='merge_queue'),
                     dict(type='pull_request', parameters=dict(required_approving_review_count=1)),
                     dict(type='required_status_checks', parameters=dict(required_status_checks=[
                         dict(context='test'), dict(context='workflows')]))]
        pr = dict(state='OPEN', isDraft=False, baseRefName='main',
                  headRefOid='a' * 40, autoMergeRequest=None)
        pr.update(changes or {})
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            log = root / 'calls'
            gh = root / 'gh'
            gh.write_text('#!/usr/bin/env python3\nimport json,os,sys\n'
                          'with open(os.environ["CALLS"],"a") as f: f.write(json.dumps(sys.argv[1:])+"\\n")\n'
                          'if sys.argv[1] == "api":\n'
                          ' print(os.environ["RULES"])\n'
                          ' sys.exit(0)\n'
                          'if sys.argv[1:3] == ["pr","view"]:\n'
                          ' print(os.environ["PR_DATA"])\n'
                          ' sys.exit(int(os.environ["VIEW_EXIT"]))\n'
                          'sys.exit(int(os.environ["MERGE_EXIT"]))\n')
            gh.chmod(0o700)
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}',
                       GH_REPO='AgentsHangout/agentshangout', PR_NUMBER='123',
                       EXPECTED_HEAD='a' * 40, CALLS=str(log),
                       PR_DATA=json.dumps(pr), RULES=json.dumps(rules), VIEW_EXIT=str(view_exit), MERGE_EXIT=str(merge_exit))
            env.update(env_changes or {})
            result = subprocess.run(['python3', str(SCRIPT)], env=env, capture_output=True, text=True)
            calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            return result, calls

    def test_ready_pr_enrolls_without_admin_bypass_and_matches_head(self):
        result, calls = self.run_case()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls[-1], ['pr', 'merge', '123', '--repo', 'AgentsHangout/agentshangout',
                                    '--auto', '--squash', '--match-head-commit', 'a' * 40])

    def test_ineligible_or_already_enabled_pr_does_not_write(self):
        for change in [dict(isDraft=True), dict(state='CLOSED'), dict(state='MERGED'),
                       dict(baseRefName='release'), dict(headRefOid='b' * 40),
                       dict(autoMergeRequest={'enabledAt': 'now'})]:
            with self.subTest(change=change):
                result, calls = self.run_case(change)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(len(calls), 1)

    def test_missing_queue_policy_fails_closed(self):
        for rules in [[], [dict(type='merge_queue')]]:
            with self.subTest(rules=rules):
                result, calls = self.run_case(rules=rules)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(any(call[:2] == ['pr', 'merge'] for call in calls))

    def test_manual_dispatch_uses_current_head(self):
        result, calls = self.run_case(env_changes=dict(EXPECTED_HEAD=''))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls[-1][-2:], ['--match-head-commit', 'a' * 40])

    def test_bad_event_identifiers_fail_before_any_api_call(self):
        for env in [dict(PR_NUMBER='123; touch nope'), dict(PR_NUMBER='0'),
                    dict(EXPECTED_HEAD='main'), dict(GH_REPO='--help')]:
            with self.subTest(env=env):
                result, calls = self.run_case(env_changes=env)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(calls, [])

    def test_read_failure_never_attempts_merge(self):
        result, calls = self.run_case(view_exit=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(calls), 1)

    def test_write_failure_is_not_reported_as_success_or_retried(self):
        result, calls = self.run_case(merge_exit=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(calls), 3)

    def test_missing_required_response_fields_fail_closed(self):
        result, calls = self.run_case(dict(isDraft=None))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(calls), 1)


class WorkflowContractTests(unittest.TestCase):
    def test_ci_keeps_pr_and_queue_triggers_and_independent_groups(self):
        ci = (ROOT / '.github/workflows/ci.yml').read_text()
        self.assertIn('  pull_request:', ci)
        self.assertIn('  merge_group:', ci)
        self.assertIn('types: [checks_requested]', ci)
        self.assertIn('github.event.pull_request.number || github.ref', ci)
        self.assertIn('  test:', ci)
        self.assertIn('  workflows:', ci)
        self.assertIn('python3 -m unittest discover -s .github/tests -v', ci)
        self.assertNotIn('  pull_request_target:', ci)

    def test_enrollment_uses_trusted_base_without_persisted_credentials(self):
        workflow = (ROOT / '.github/workflows/auto-merge.yml').read_text()
        self.assertIn("vars.MERGE_QUEUE_ENABLED == 'true'", workflow)
        self.assertIn('github.event.pull_request.base.sha || github.sha', workflow)
        self.assertIn('persist-credentials: false', workflow)
        self.assertNotIn('ref: ${{ github.event.pull_request.head', workflow)
        self.assertNotIn('--admin', workflow)
        self.assertNotIn('write-all', workflow)


if __name__ == '__main__':
    unittest.main()
