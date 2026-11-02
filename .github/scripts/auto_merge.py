"""Enroll one PR using trusted metadata, leaving all policy enforcement to GitHub."""
import json
import os
import re
import subprocess


def main():
    number = os.environ['PR_NUMBER']
    repo = os.environ['GH_REPO']
    expected = os.environ.get('EXPECTED_HEAD', '')
    if not re.fullmatch(r'[1-9][0-9]*', number):
        raise ValueError('Invalid PR number')
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repo):
        raise ValueError('Invalid repository')
    if expected and not re.fullmatch(r'[0-9a-f]{40}', expected):
        raise ValueError('Invalid event head')
    result = subprocess.run(['gh', 'pr', 'view', number, '--repo', repo, '--json',
                             'state,isDraft,baseRefName,headRefOid,autoMergeRequest'],
                            check=True, capture_output=True, text=True)
    pr = json.loads(result.stdout)
    if not isinstance(pr['isDraft'], bool):
        raise ValueError('Missing draft status')
    head = pr['headRefOid']
    if not isinstance(head, str) or not re.fullmatch(r'[0-9a-f]{40}', head):
        raise ValueError('Invalid current head')
    if (pr['state'] != 'OPEN' or pr['isDraft'] or pr['baseRefName'] != 'main'
            or (expected and head != expected) or pr['autoMergeRequest'] is not None):
        print('Skipped: PR is ineligible, event is stale, or auto-merge is already enabled.')
        return
    # Fail closed when the rollout is incomplete or its policy has been removed.
    rules_result = subprocess.run(['gh', 'api', f'repos/{repo}/rules/branches/main'],
                                  check=True, capture_output=True, text=True)
    rules = json.loads(rules_result.stdout)
    types = {rule['type'] for rule in rules}
    approvals = any(rule['type'] == 'pull_request'
                    and rule.get('parameters', {}).get('required_approving_review_count', 0) >= 1
                    for rule in rules)
    checks = {check['context'] for rule in rules if rule['type'] == 'required_status_checks'
              for check in rule.get('parameters', {}).get('required_status_checks', [])}
    if 'merge_queue' not in types or not approvals or not {'test', 'workflows'} <= checks:
        raise ValueError('Main must require merge queue, a review, test and workflows before enrollment')
    # No shell interpolation, no admin bypass, and no retry after an uncertain write.
    subprocess.run(['gh', 'pr', 'merge', number, '--repo', repo, '--auto', '--squash',
                    '--match-head-commit', head], check=True)
    print('GitHub accepted queue/auto-merge enrollment; merge and deployment are not yet verified.')


if __name__ == '__main__':
    main()
