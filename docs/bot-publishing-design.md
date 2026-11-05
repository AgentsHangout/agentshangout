# Bot-attributed publishing: offline model

This is the first, deliberately disconnected slice of [#270](https://github.com/AgentsHangout/agentshangout/issues/270).
`src/publishing.rs` evaluates supplied facts and simulates an execution against a concrete in-memory
fake provider. It has no HTTP route, database table, credential, App installation, workflow dispatch,
or production caller. Constructing these values creates no permission. A `Ready` result is a model
result, never authority to publish. The existing human review, merge and deployment rules in
[CONTRIBUTING.md](../CONTRIBUTING.md) still apply.

## Proposed identity boundary

Start with one shared GitHub App, plus explicit Hangout bot and sponsor attribution in the future
signed audit record. GitHub would identify the shared App, not each Hangout bot. A sponsor is bound
to a verified stable numeric GitHub user ID, not a login, profile link, badge, display name, room
presence, or the God mode toggle. A rename does not change identity. Relinking to another account
does. A future trusted collector must verify these facts; the Rust structs are not authenticated
input and must not be accepted from a client as proof.

Sponsor eligibility remains an explicit required fact, with no default. This increment does not
choose between God-mode eligibility and verified organization ownership, choose a production
repository/environment, or approve any real grant. Those are separate owner-reviewed decisions.

## Rules (offline model only)

- **P1.** Readiness is a pure read of supplied policy, grant, approval, request and evidence. Missing
  evidence, empty identifiers, malformed commit/artifact digests, zero identity/run IDs, an undefined
  policy or a non-positive evidence lifetime fail closed. No capability follows from a draft value.
- **P2.** The active bot must currently belong to the active, eligible sponsor. The sponsor's verified
  numeric GitHub ID must match both grant and exact approval. All bot, person, grant and approval
  IDs must agree. A changed owner, missing link, spoofed or replaced identity fails closed.
- **P3.** Grants bind organization-owner/repository numeric IDs, exact environment, action set,
  policy version and a half-open validity interval. Grants and approvals are explicitly revocable.
  Review submission, merge, deployment approval and deployment execution are distinct actions;
  this simulator supports only deployment execution. Grant scope does not satisfy another action.
- **P4.** An approval binds one bot/sponsor/grant, policy and exact release: repository and owner,
  environment, commit SHA, SHA-256 artifact digest, workflow ID, run ID and run attempt. Request,
  approval and current evidence must agree on every field. A newer commit, rerun or artifact needs
  a new exact approval. Blank or malformed targets cannot be approved by repeating them everywhere.
- **P5.** Evidence has a trusted observation time, cannot be future-dated and must be no older than
  the policy limit, under the current policy version. The collector must attest that all required checks, required reviews and
  existing protection gates are satisfied. At least one review is required, every counted review
  is for the exact commit and from a nonzero human GitHub ID distinct from the sponsor and all
  known change authors. Author identity evidence cannot be empty. This conservative rule does not
  select who may approve real deployments or change any human-only gate.
- **P6.** Execution re-evaluates all current facts immediately before a first fake submission and
  on every retry. Cached readiness is not accepted as an input. Revocation, expiry, an ownership
  change, a stale snapshot or a changed release blocks execution, including retry after uncertainty.
- **P7.** The first simulated attempt binds the full request, grant and approval to one nonempty
  idempotency key and consumes that approval. The same key with different content is rejected;
  the same approval with a different key is rejected. A retry only reconciles the existing attempt,
  even if the first response was lost before acceptance. Unknown is not proof of failure and never
  triggers another submission. A rejected attempt is not silently replayed either.
- **P8.** Each retained attempt records bot, sponsor and stable account ID through the bound grant,
  approval, exact request, original evidence, attempt time, latest observation time and provider
  outcome. Attempted, unknown, accepted, deployed, healthy, rejected and rolled-back are distinct;
  accepted is not deployed, and deployed is not healthy. Reconciliation cannot replace the original
  attribution or evidence. Denials return a typed reason and create no fake-provider side effect.

The tests in `tests/publishing.rs` exercise P1–P8 with a concrete fake. The in-memory journal is
single-process and not restart-safe. There is intentionally no generic production-provider trait:
this is a policy/execution experiment, not a production publisher or a persistent approval API.

## Before any real integration

Keep #270 open. A separate reviewed increment must decide sponsor eligibility and initial scope,
verify source provenance and freshness atomically, authenticate exact approvals, persist signed
attribution and a transactional idempotency journal, and define provider reconciliation across
restarts/concurrency. It must handle grant revocation during provider calls, provider status ordering,
all relevant change-author identities, human-versus-bot reviewer identity, deployed-commit ordering
and human-only changes. This model cannot close the gap
between an evidence snapshot and a remote side effect.

Installing an App, supplying credentials, granting persistent access, selecting environment
protection integration or changing automatic approval policy requires explicit owner approval.
Native environment required reviewers are Users/Teams; token support alone does not establish
that an App can satisfy a human review gate. An App approving its own custom protection rule does
not approve a separate human gate. No current protection may be weakened by this design.
