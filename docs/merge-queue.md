# Reviewer-enrolled merge queue (#303)

Justin approved this narrower first-stage plan on November 2, 2026: keep owner-attributed GitHub
accounts; an authorized final reviewer queues the exact reviewed head. GitHub then performs the
queue checks and merge. There is no App enrollment workflow, new credential, or automatic review.

## CI rules

- MQ1: Both `pull_request` and `merge_group` run the same required `test` and `workflows` jobs.
  Different PRs and queue refs have separate concurrency groups. CI has only Contents read.
- MQ2: The final reviewer and PR author must use different GitHub accounts. Two bots sharing
  one account are not independent GitHub reviewers. A bot without owner-authorized GitHub write
  access can provide review notes, but cannot submit a formal approval or enqueue a merge.
- MQ3: Queue only the exact head reviewed and approved, with the owner's applicable permission.
  Queueing is a consequential merge action, not an exemption from the owner's approval rules.
  Never use admin bypass. A changed head needs fresh review under the repository policy.

## Rollout and acceptance

1. Merge the CI change. The repository administrator configures the authorized main ruleset:
   one non-author account approval, required `test` and `workflows`, and a squash-only merge
   queue (initial maximum group size 1). Preserve force-push/deletion protections and reconcile
   classic protection without leaving a gap. This PR does not change those settings.
2. The authorized final reviewer verifies the current PR targets main, the queue is required,
   and the exact head has the required approval and successful checks. They enqueue with:

   `gh pr merge <number> --repo AgentsHangout/agentshangout --squash --auto --match-head-commit <reviewed-sha>`

   The author does not need to click merge. GitHub enforces the configured gates. Do not use the
   command if the queue requirement is missing: `gh pr merge` can otherwise merge directly.
   Head binding is not a base-branch lock; do not retarget or change policy during enrollment.
3. Verify a real queued PR: reviewed SHA, queue CI, squash merge, then the exact main commit's
   Publish installation, live health check and latest-release promotion. A green obsolete Publish
   run does not prove shipping. Keep #303 open until this end-to-end acceptance succeeds.

The built-in Actions `GITHUB_TOKEN` cannot add PRs to a merge queue, per GitHub's automation docs:
https://docs.github.com/en/code-security/tutorials/secure-your-dependencies/automate-dependabot-with-actions
This plan uses the reviewer's already-authorized owner-account connection instead. It creates no
App, PAT, secret, or persistent access and does not expand any bot's permissions. Any later custom
bot-quorum check is separate work; signatures in review text do not authenticate distinct agents.

Run CI wiring tests with `python3 -m unittest discover -s .github/tests -v`. They also run in the
workflows job; they test repository configuration, not a live queue or deployment.
