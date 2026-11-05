//! P1–P8: disconnected readiness and fake-provider execution. No real grant or workflow is created.

use agenthangout::publishing::{
    readiness, Action, Approval, Denial, Evidence, FakeResponse, Grant, Outcome, Policy, Ready,
    Release, Request, Review, Simulation, Sponsor,
};

const NOW: i64 = 1_000;

#[derive(Clone)]
struct Fixture {
    policy: Policy,
    grant: Grant,
    approval: Approval,
    request: Request,
    evidence: Evidence,
}

impl Fixture {
    fn new() -> Self {
        let release = Release {
            owner_id: 11,
            repository_id: 22,
            environment: "simulation".into(),
            commit_sha: "a".repeat(40),
            artifact_sha256: "b".repeat(64),
            workflow_id: 33,
            run_id: 44,
            run_attempt: 1,
        };
        Self {
            policy: Policy { version: "test-policy-1".into(), max_evidence_age_seconds: 30 },
            grant: Grant {
                id: "grant-1".into(),
                bot_id: "bot-1".into(),
                sponsor_person_id: "person-1".into(),
                sponsor_github_id: 101,
                owner_id: release.owner_id,
                repository_id: release.repository_id,
                environment: release.environment.clone(),
                actions: vec![Action::ExecuteDeployment],
                policy_version: "test-policy-1".into(),
                valid_from: NOW - 100,
                expires_at: NOW + 100,
                revoked: false,
            },
            approval: Approval {
                id: "approval-1".into(),
                grant_id: "grant-1".into(),
                bot_id: "bot-1".into(),
                sponsor_person_id: "person-1".into(),
                sponsor_github_id: 101,
                policy_version: "test-policy-1".into(),
                action: Action::ExecuteDeployment,
                release: release.clone(),
                valid_from: NOW - 50,
                expires_at: NOW + 50,
                revoked: false,
            },
            request: Request {
                idempotency_key: "attempt-1".into(),
                approval_id: "approval-1".into(),
                bot_id: "bot-1".into(),
                action: Action::ExecuteDeployment,
                release: release.clone(),
            },
            evidence: Evidence {
                observed_at: NOW,
                policy_version: "test-policy-1".into(),
                sponsor: Sponsor {
                    person_id: "person-1".into(),
                    verified_github_id: Some(101),
                    active: true,
                    eligible: true,
                },
                bot_id: "bot-1".into(),
                bot_owner_person_id: "person-1".into(),
                bot_active: true,
                current_release: release,
                required_checks_passed: true,
                required_reviews_satisfied: true,
                existing_protections_satisfied: true,
                change_author_github_ids: vec![202],
                reviews: vec![Review { github_user_id: 303, approved_commit_sha: "a".repeat(40) }],
            },
        }
    }

    fn ready(&self) -> Result<Ready, Denial> {
        readiness(
            &self.policy,
            &self.grant,
            &self.approval,
            &self.request,
            Some(&self.evidence),
            NOW,
        )
    }

    fn execute(
        &self,
        simulation: &mut Simulation,
    ) -> Result<agenthangout::publishing::Attempt, Denial> {
        simulation.execute(
            &self.policy,
            &self.grant,
            &self.approval,
            &self.request,
            Some(&self.evidence),
            NOW,
        )
    }
}

#[test]
fn p1_readiness_is_repeatable_and_does_not_create_an_attempt() {
    let fixture = Fixture::new();
    let simulation = Simulation::default();
    assert_eq!(fixture.ready(), Ok(Ready));
    assert_eq!(fixture.ready(), Ok(Ready));
    assert_eq!(simulation.submissions(), 0);
    assert!(simulation.attempt("attempt-1").is_none());
    assert_eq!(
        readiness(&fixture.policy, &fixture.grant, &fixture.approval, &fixture.request, None, NOW,),
        Err(Denial::MissingEvidence)
    );
}

#[test]
fn p1_to_p6_invalid_or_changed_facts_fail_closed_before_submission() {
    type Case = (&'static str, fn(&mut Fixture), Denial);
    let cases: Vec<Case> = vec![
        ("undefined policy", |f| f.policy.version.clear(), Denial::InvalidInput),
        ("zero evidence lifetime", |f| f.policy.max_evidence_age_seconds = 0, Denial::InvalidInput),
        ("empty grant", |f| f.grant.id.clear(), Denial::InvalidInput),
        ("blank bot", |f| f.grant.bot_id = " ".into(), Denial::InvalidInput),
        ("empty sponsor", |f| f.grant.sponsor_person_id.clear(), Denial::InvalidInput),
        ("zero sponsor account", |f| f.grant.sponsor_github_id = 0, Denial::InvalidInput),
        ("empty approval", |f| f.approval.id.clear(), Denial::InvalidInput),
        ("blank retry key", |f| f.request.idempotency_key = " ".into(), Denial::InvalidInput),
        ("future snapshot", |f| f.evidence.observed_at = NOW + 1, Denial::StaleEvidence),
        ("old snapshot", |f| f.evidence.observed_at = NOW - 31, Denial::StaleEvidence),
        ("snapshot age overflow", |f| f.evidence.observed_at = i64::MIN, Denial::StaleEvidence),
        (
            "old snapshot policy",
            |f| f.evidence.policy_version = "old".into(),
            Denial::StaleEvidence,
        ),
        ("ineligible sponsor", |f| f.evidence.sponsor.eligible = false, Denial::IneligibleSponsor),
        ("inactive sponsor", |f| f.evidence.sponsor.active = false, Denial::IneligibleSponsor),
        ("deleted bot", |f| f.evidence.bot_active = false, Denial::IneligibleSponsor),
        (
            "unlinked account",
            |f| f.evidence.sponsor.verified_github_id = None,
            Denial::IdentityMismatch,
        ),
        (
            "relinked account",
            |f| f.evidence.sponsor.verified_github_id = Some(999),
            Denial::IdentityMismatch,
        ),
        (
            "wrong person",
            |f| f.evidence.sponsor.person_id = "other".into(),
            Denial::IdentityMismatch,
        ),
        (
            "changed bot owner",
            |f| f.evidence.bot_owner_person_id = "other".into(),
            Denial::IdentityMismatch,
        ),
        ("other bot facts", |f| f.evidence.bot_id = "other".into(), Denial::IdentityMismatch),
        ("cross-bot request", |f| f.request.bot_id = "other".into(), Denial::IdentityMismatch),
        ("revoked grant", |f| f.grant.revoked = true, Denial::InvalidGrant),
        ("future grant", |f| f.grant.valid_from = NOW + 1, Denial::InvalidGrant),
        ("expired grant", |f| f.grant.expires_at = NOW, Denial::InvalidGrant),
        ("old grant policy", |f| f.grant.policy_version = "old".into(), Denial::InvalidGrant),
        ("revoked approval", |f| f.approval.revoked = true, Denial::InvalidApproval),
        ("future approval", |f| f.approval.valid_from = NOW + 1, Denial::InvalidApproval),
        ("expired approval", |f| f.approval.expires_at = NOW, Denial::InvalidApproval),
        (
            "old approval policy",
            |f| f.approval.policy_version = "old".into(),
            Denial::InvalidApproval,
        ),
        ("other approval grant", |f| f.approval.grant_id = "other".into(), Denial::InvalidApproval),
        ("other approval bot", |f| f.approval.bot_id = "other".into(), Denial::InvalidApproval),
        (
            "other approval sponsor",
            |f| f.approval.sponsor_person_id = "other".into(),
            Denial::InvalidApproval,
        ),
        ("other approval account", |f| f.approval.sponsor_github_id = 999, Denial::InvalidApproval),
        (
            "other approval request",
            |f| f.request.approval_id = "other".into(),
            Denial::InvalidApproval,
        ),
        ("other approval action", |f| f.approval.action = Action::Merge, Denial::InvalidApproval),
        ("other grant owner", |f| f.grant.owner_id += 1, Denial::ScopeMismatch),
        ("other grant repository", |f| f.grant.repository_id += 1, Denial::ScopeMismatch),
        (
            "other grant environment",
            |f| f.grant.environment = "production".into(),
            Denial::ScopeMismatch,
        ),
        ("missing action", |f| f.grant.actions.clear(), Denial::ScopeMismatch),
        (
            "pending checks",
            |f| f.evidence.required_checks_passed = false,
            Denial::RequiredEvidenceMissing,
        ),
        (
            "pending reviews",
            |f| f.evidence.required_reviews_satisfied = false,
            Denial::RequiredEvidenceMissing,
        ),
        (
            "human gate pending",
            |f| f.evidence.existing_protections_satisfied = false,
            Denial::RequiredEvidenceMissing,
        ),
        ("no reviews", |f| f.evidence.reviews.clear(), Denial::RequiredEvidenceMissing),
        (
            "no authors",
            |f| f.evidence.change_author_github_ids.clear(),
            Denial::RequiredEvidenceMissing,
        ),
        (
            "unknown author",
            |f| f.evidence.change_author_github_ids.push(0),
            Denial::RequiredEvidenceMissing,
        ),
        (
            "sponsor review",
            |f| f.evidence.reviews[0].github_user_id = 101,
            Denial::ReviewNotIndependent,
        ),
        (
            "author review",
            |f| f.evidence.reviews[0].github_user_id = 202,
            Denial::ReviewNotIndependent,
        ),
        (
            "unknown reviewer",
            |f| f.evidence.reviews[0].github_user_id = 0,
            Denial::ReviewNotIndependent,
        ),
        (
            "old review",
            |f| f.evidence.reviews[0].approved_commit_sha = "c".repeat(40),
            Denial::ReviewNotIndependent,
        ),
    ];
    for (name, change, expected) in cases {
        let mut fixture = Fixture::new();
        change(&mut fixture);
        assert_eq!(fixture.ready(), Err(expected), "{name}");
        let mut simulation = Simulation::default();
        assert_eq!(fixture.execute(&mut simulation), Err(expected), "{name}");
        assert_eq!(simulation.submissions(), 0, "{name}");
        assert!(simulation.attempt("attempt-1").is_none(), "{name}");
    }
}

#[test]
fn p3_review_merge_and_deployment_approval_are_not_execution() {
    for action in [Action::SubmitReview, Action::Merge, Action::ApproveDeployment] {
        let mut fixture = Fixture::new();
        fixture.request.action = action;
        fixture.approval.action = action;
        fixture.grant.actions.push(action);
        assert_eq!(fixture.ready(), Err(Denial::UnsupportedAction));
    }
}

#[test]
fn p4_every_release_field_is_bound_to_approval_and_current_evidence() {
    let changes: Vec<fn(&mut Release)> = vec![
        |r| r.owner_id += 1,
        |r| r.repository_id += 1,
        |r| r.environment = "another-environment".into(),
        |r| r.commit_sha = "c".repeat(40),
        |r| r.artifact_sha256 = "d".repeat(64),
        |r| r.workflow_id += 1,
        |r| r.run_id += 1,
        |r| r.run_attempt += 1,
    ];
    for change in changes {
        let mut fixture = Fixture::new();
        change(&mut fixture.approval.release);
        assert_eq!(fixture.ready(), Err(Denial::ReleaseMismatch));
        let mut fixture = Fixture::new();
        change(&mut fixture.evidence.current_release);
        assert_eq!(fixture.ready(), Err(Denial::ReleaseMismatch));
    }
}

#[test]
fn p1_and_p4_equal_but_malformed_targets_still_fail_closed() {
    let changes: Vec<fn(&mut Release)> = vec![
        |r| r.owner_id = 0,
        |r| r.repository_id = 0,
        |r| r.environment.clear(),
        |r| r.environment = " simulation".into(),
        |r| r.commit_sha = "a".repeat(7),
        |r| r.commit_sha = "z".repeat(40),
        |r| r.artifact_sha256 = "b".repeat(63),
        |r| r.artifact_sha256 = "z".repeat(64),
        |r| r.workflow_id = 0,
        |r| r.run_id = 0,
        |r| r.run_attempt = 0,
    ];
    for change in changes {
        let mut fixture = Fixture::new();
        change(&mut fixture.request.release);
        fixture.approval.release = fixture.request.release.clone();
        fixture.evidence.current_release = fixture.request.release.clone();
        assert_eq!(fixture.ready(), Err(Denial::InvalidInput));
    }
}

#[test]
fn p5_freshness_and_validity_boundaries_are_explicit() {
    let mut fixture = Fixture::new();
    fixture.evidence.observed_at = NOW - 30;
    fixture.grant.valid_from = NOW;
    fixture.approval.valid_from = NOW;
    assert_eq!(fixture.ready(), Ok(Ready));
    fixture.evidence.observed_at -= 1;
    assert_eq!(fixture.ready(), Err(Denial::StaleEvidence));
}

#[test]
fn p6_readiness_cannot_be_cached_across_revocation_or_an_ownership_change() {
    let mut fixture = Fixture::new();
    assert_eq!(fixture.ready(), Ok(Ready));
    fixture.grant.revoked = true;
    let mut simulation = Simulation::default();
    assert_eq!(fixture.execute(&mut simulation), Err(Denial::InvalidGrant));
    assert_eq!(simulation.submissions(), 0);

    let mut fixture = Fixture::new();
    simulation.next_response = FakeResponse::LoseResponseAfterAccept;
    assert_eq!(fixture.execute(&mut simulation).unwrap().outcome, Outcome::Unknown);
    fixture.approval.revoked = true;
    assert_eq!(fixture.execute(&mut simulation), Err(Denial::InvalidApproval));
    fixture.approval.revoked = false;
    fixture.evidence.bot_owner_person_id = "new-owner".into();
    assert_eq!(fixture.execute(&mut simulation), Err(Denial::IdentityMismatch));
    assert_eq!(simulation.submissions(), 1);
    assert_eq!(simulation.attempt("attempt-1").unwrap().outcome, Outcome::Unknown);
}

#[test]
fn p7_lost_responses_are_reconciled_without_another_submission() {
    for (response, reconciled) in [
        (FakeResponse::LoseResponseAfterAccept, Outcome::Accepted),
        (FakeResponse::LoseResponseBeforeAccept, Outcome::Unknown),
        (FakeResponse::Reject, Outcome::Rejected),
        (FakeResponse::Accept, Outcome::Accepted),
    ] {
        let fixture = Fixture::new();
        let mut simulation = Simulation::default();
        simulation.next_response = response;
        fixture.execute(&mut simulation).unwrap();
        for _ in 0..3 {
            assert_eq!(fixture.execute(&mut simulation).unwrap().outcome, reconciled);
        }
        assert_eq!(simulation.submissions(), 1);
    }
}

#[test]
fn p7_changed_keys_and_changed_intent_cannot_replay_an_approval() {
    let mut fixture = Fixture::new();
    let mut simulation = Simulation::default();
    fixture.execute(&mut simulation).unwrap();
    fixture.request.idempotency_key = "second-key".into();
    assert_eq!(fixture.execute(&mut simulation), Err(Denial::ApprovalReplay));
    fixture.request.idempotency_key = "attempt-1".into();
    fixture.request.release.run_attempt += 1;
    fixture.approval.release = fixture.request.release.clone();
    fixture.evidence.current_release = fixture.request.release.clone();
    assert_eq!(fixture.ready(), Ok(Ready));
    assert_eq!(fixture.execute(&mut simulation), Err(Denial::IdempotencyConflict));
    assert_eq!(simulation.submissions(), 1);
}

#[test]
fn p7_reusing_a_key_with_a_new_approval_or_changed_grant_is_rejected() {
    let fixture = Fixture::new();
    let mut simulation = Simulation::default();
    fixture.execute(&mut simulation).unwrap();
    let mut changed = fixture.clone();
    changed.approval.id = "new-approval".into();
    changed.request.approval_id = changed.approval.id.clone();
    assert_eq!(changed.execute(&mut simulation), Err(Denial::IdempotencyConflict));
    let mut changed = fixture;
    changed.grant.expires_at += 1;
    assert_eq!(changed.execute(&mut simulation), Err(Denial::IdempotencyConflict));
    assert_eq!(simulation.submissions(), 1);
}

#[test]
fn p8_audit_keeps_original_attribution_evidence_and_distinct_outcomes() {
    let mut fixture = Fixture::new();
    let mut simulation = Simulation::default();
    let first = fixture.execute(&mut simulation).unwrap();
    assert_eq!(first.outcome, Outcome::Accepted);
    assert_eq!(first.request, fixture.request);
    assert_eq!(first.grant, fixture.grant);
    assert_eq!(first.approval, fixture.approval);
    assert_eq!(first.original_evidence, fixture.evidence);
    assert_eq!(first.attempted_at, NOW);
    fixture.evidence.observed_at -= 1;
    for outcome in [Outcome::Deployed, Outcome::Healthy, Outcome::RolledBack] {
        assert!(simulation.observe_provider("attempt-1", outcome));
        let attempt = fixture.execute(&mut simulation).unwrap();
        assert_eq!(attempt.outcome, outcome);
        assert_eq!(attempt.original_evidence, first.original_evidence);
        assert_eq!(attempt.grant.sponsor_github_id, 101);
        assert_eq!(attempt.grant.bot_id, "bot-1");
        assert_eq!(attempt.grant.sponsor_person_id, "person-1");
    }
    assert!(!simulation.observe_provider("unsubmitted", Outcome::Healthy));
    assert_eq!(simulation.submissions(), 1);
}
