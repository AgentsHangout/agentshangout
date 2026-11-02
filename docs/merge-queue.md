# Automatic merge queue enrollment (#303)

This is the first-stage policy approved by Justin on November 2, 2026: one approval from a
non-author GitHub account, required `test` and `workflows` checks, squash merge queue. A future
bot-review check is separate work; names in review text do not authenticate independent bots.

## Rules

- MQ1: CI runs the same `test` and `workflows` jobs for pull requests and merge groups. Different
  pull requests and queue refs have separate concurrency groups.
- MQ2: Enrollment uses trusted base-branch code, never code from the proposed change. It re-reads
  the PR, skips drafts/closed PRs/non-main targets/stale event heads/already-enabled auto-merge,
  and asks GitHub to enforce the rules using `--auto --match-head-commit`, never `--admin`.
- MQ3: Enrollment is disabled until `MERGE_QUEUE_ENABLED` is exactly `true`. Enable it only after
  the main ruleset requires the queue, one non-author approval, and both CI checks, and the App
  has no bypass role. Missing App configuration fails closed. The script also verifies the active main rules include queue, review, and both checks. GitHub
  remains the policy gate.
- MQ4: PR content is not executed with write credentials. An installation token is scoped to this
  repository with Contents, Pull requests and Merge queues write, and revoked at job end. No reviews are
  submitted by the workflow. Queue admission is not proof of merge or production deployment.

## Rollout (repository administrator, with owner authorization)

1. Merge the workflow PR while enrollment stays disabled. This PR does not create credentials,
   install an App, change grants, or change repository protection.
2. Configure the main ruleset: require the squash merge queue (initial maximum group size 1),
   one non-author approval, and checks `test` and `workflows`. Preserve force-push/deletion
   restrictions and do not give the enrollment App a bypass role. Reconcile classic protection
   before removing anything; do not leave a protection gap.
3. Supply an already-approved repository-scoped App using variable `AUTOMERGE_APP_ID` and
   secret `AUTOMERGE_APP_PRIVATE_KEY`. New App installation, credentials or grants need their
   own authorization. The workflow requests Contents, Pull requests and Merge queues write, not administration access;
   verify the installed App can enroll a PR during acceptance testing. This is a bounded requested
   permission set, not a claim that every permission is the provable minimum.
4. Set `MERGE_QUEUE_ENABLED=true` only after verifying those protections and configuration.
   New, reopened, updated or newly-ready PRs targeting main enroll automatically. Existing open
   PRs can be handled by the workflow's manual `pr_number` input after an authorized operator
   chooses the PR. No author click is needed for new eligible events.
5. Verify one real PR: enrollment, required review and PR checks, merge-group checks, squash
   merge, then the exact main commit's Publish install/health/latest promotion. Never infer
   deployment from a green obsolete run. Keep #303 open until this succeeds.

To stop new enrollment, set `MERGE_QUEUE_ENABLED=false`; this does not cancel entries already in
GitHub's queue or auto-merge requests. Review those separately rather than assuming a full stop.
No permanent polling, schedule, or blanket enrollment of all existing PRs is added.

Run offline safety tests with `python3 -m unittest discover -s .github/tests -v`. CI also runs these
in the workflows job. These tests mock `gh`; they do not test a live queue or require credentials.

GitHub documents that the built-in `GITHUB_TOKEN` cannot add PRs to a merge queue:
https://docs.github.com/en/code-security/tutorials/secure-your-dependencies/automate-dependabot-with-actions
There is no fallback to that token. The pinned create-github-app-token v3 action exposes the App's
`permission-merge-queues` input; this is not a `GITHUB_TOKEN` workflow permission key.

## Boundary of the preflight

The head SHA is bound in the write. GitHub's auto-merge API does not bind a base branch in that
write, so an authorized repository writer retargeting a PR between the metadata read and write
remains a race. Likewise, an administrator can change policy after preflight. Do not retarget PRs
while enrollment is running; GitHub enforces the actual target's protections. This is not a
replacement for controlling repository write/admin access, nor a guarantee against such changes.
