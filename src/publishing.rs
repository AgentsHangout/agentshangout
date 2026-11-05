//! Offline publishing policy experiment (docs/bot-publishing-design.md, P1–P8).
//!
//! These are untrusted plain values, not authentication proofs. No production caller, storage,
//! network transport or credential is connected to this module. `Ready` grants no real permission.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    SubmitReview,
    Merge,
    ApproveDeployment,
    ExecuteDeployment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub owner_id: u64,
    pub repository_id: u64,
    pub environment: String,
    pub commit_sha: String,
    pub artifact_sha256: String,
    pub workflow_id: u64,
    pub run_id: u64,
    pub run_attempt: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub version: String,
    pub max_evidence_age_seconds: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub id: String,
    pub bot_id: String,
    pub sponsor_person_id: String,
    pub sponsor_github_id: u64,
    pub owner_id: u64,
    pub repository_id: u64,
    pub environment: String,
    pub actions: Vec<Action>,
    pub policy_version: String,
    pub valid_from: i64,
    pub expires_at: i64,
    pub revoked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    pub id: String,
    pub grant_id: String,
    pub bot_id: String,
    pub sponsor_person_id: String,
    pub sponsor_github_id: u64,
    pub policy_version: String,
    pub action: Action,
    pub release: Release,
    pub valid_from: i64,
    pub expires_at: i64,
    pub revoked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub idempotency_key: String,
    pub approval_id: String,
    pub bot_id: String,
    pub action: Action,
    pub release: Release,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sponsor {
    pub person_id: String,
    /// Only a trusted collector may attest this verified, stable GitHub account ID.
    pub verified_github_id: Option<u64>,
    pub active: bool,
    pub eligible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Review {
    pub github_user_id: u64,
    pub approved_commit_sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub observed_at: i64,
    pub policy_version: String,
    pub sponsor: Sponsor,
    pub bot_id: String,
    pub bot_owner_person_id: String,
    pub bot_active: bool,
    pub current_release: Release,
    pub required_checks_passed: bool,
    pub required_reviews_satisfied: bool,
    pub existing_protections_satisfied: bool,
    pub change_author_github_ids: Vec<u64>,
    /// Counted human approvals only, collected for this exact commit.
    pub reviews: Vec<Review>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denial {
    InvalidInput,
    MissingEvidence,
    StaleEvidence,
    IdentityMismatch,
    IneligibleSponsor,
    InvalidGrant,
    InvalidApproval,
    ScopeMismatch,
    UnsupportedAction,
    ReleaseMismatch,
    RequiredEvidenceMissing,
    ReviewNotIndependent,
    IdempotencyConflict,
    ApprovalReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ready;

fn present(value: &str) -> bool {
    !value.trim().is_empty() && value.trim() == value
}

fn digest(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_release(release: &Release) -> bool {
    release.owner_id != 0
        && release.repository_id != 0
        && present(&release.environment)
        && digest(&release.commit_sha, 40)
        && digest(&release.artifact_sha256, 64)
        && release.workflow_id != 0
        && release.run_id != 0
        && release.run_attempt != 0
}

/// P1–P5: read-only eligibility for a supplied snapshot, not permission or a cached execution token.
pub fn readiness(
    policy: &Policy,
    grant: &Grant,
    approval: &Approval,
    request: &Request,
    evidence: Option<&Evidence>,
    now: i64,
) -> Result<Ready, Denial> {
    if !present(&policy.version)
        || policy.max_evidence_age_seconds <= 0
        || !present(&grant.id)
        || !present(&grant.bot_id)
        || !present(&grant.sponsor_person_id)
        || grant.sponsor_github_id == 0
        || !present(&approval.id)
        || !present(&request.idempotency_key)
        || !valid_release(&request.release)
    {
        return Err(Denial::InvalidInput);
    }
    let evidence = evidence.ok_or(Denial::MissingEvidence)?;
    let age = now.checked_sub(evidence.observed_at).ok_or(Denial::StaleEvidence)?;
    if age < 0 || age > policy.max_evidence_age_seconds || evidence.policy_version != policy.version
    {
        return Err(Denial::StaleEvidence);
    }
    if !evidence.sponsor.active || !evidence.sponsor.eligible || !evidence.bot_active {
        return Err(Denial::IneligibleSponsor);
    }
    if evidence.sponsor.person_id != grant.sponsor_person_id
        || evidence.sponsor.verified_github_id != Some(grant.sponsor_github_id)
        || evidence.bot_owner_person_id != grant.sponsor_person_id
        || evidence.bot_id != grant.bot_id
        || request.bot_id != grant.bot_id
    {
        return Err(Denial::IdentityMismatch);
    }
    if grant.revoked
        || now < grant.valid_from
        || now >= grant.expires_at
        || grant.policy_version != policy.version
    {
        return Err(Denial::InvalidGrant);
    }
    if approval.revoked
        || now < approval.valid_from
        || now >= approval.expires_at
        || approval.policy_version != policy.version
        || approval.grant_id != grant.id
        || approval.bot_id != grant.bot_id
        || approval.sponsor_person_id != grant.sponsor_person_id
        || approval.sponsor_github_id != grant.sponsor_github_id
        || approval.id != request.approval_id
        || approval.action != request.action
    {
        return Err(Denial::InvalidApproval);
    }
    if request.action != Action::ExecuteDeployment {
        return Err(Denial::UnsupportedAction);
    }
    if grant.owner_id != request.release.owner_id
        || grant.repository_id != request.release.repository_id
        || grant.environment != request.release.environment
        || !grant.actions.contains(&request.action)
    {
        return Err(Denial::ScopeMismatch);
    }
    if approval.release != request.release || evidence.current_release != request.release {
        return Err(Denial::ReleaseMismatch);
    }
    if !evidence.required_checks_passed
        || !evidence.required_reviews_satisfied
        || !evidence.existing_protections_satisfied
        || evidence.reviews.is_empty()
        || evidence.change_author_github_ids.is_empty()
        || evidence.change_author_github_ids.contains(&0)
    {
        return Err(Denial::RequiredEvidenceMissing);
    }
    if evidence.reviews.iter().any(|review| {
        review.github_user_id == 0
            || review.github_user_id == grant.sponsor_github_id
            || evidence.change_author_github_ids.contains(&review.github_user_id)
            || review.approved_commit_sha != request.release.commit_sha
    }) {
        return Err(Denial::ReviewNotIndependent);
    }
    Ok(Ready)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Attempted,
    Unknown,
    Accepted,
    Deployed,
    Healthy,
    Rejected,
    RolledBack,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub request: Request,
    pub grant: Grant,
    pub approval: Approval,
    pub original_evidence: Evidence,
    pub attempted_at: i64,
    pub last_observed_at: i64,
    pub outcome: Outcome,
}

/// Deterministic failure injection, without a real execution adapter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FakeResponse {
    #[default]
    Accept,
    LoseResponseAfterAccept,
    LoseResponseBeforeAccept,
    Reject,
}

/// P6–P8: single-process fake only. There is deliberately no network provider or durable journal.
#[derive(Default)]
pub struct Simulation {
    attempts: HashMap<String, Attempt>,
    provider_results: HashMap<String, Outcome>,
    submissions: usize,
    pub next_response: FakeResponse,
}

impl Simulation {
    pub fn submissions(&self) -> usize {
        self.submissions
    }

    pub fn attempt(&self, key: &str) -> Option<&Attempt> {
        self.attempts.get(key)
    }

    /// Change a known fake provider result, never initiate an execution.
    pub fn observe_provider(&mut self, key: &str, outcome: Outcome) -> bool {
        if let Some(current) = self.provider_results.get_mut(key) {
            *current = outcome;
            true
        } else {
            false
        }
    }

    pub fn execute(
        &mut self,
        policy: &Policy,
        grant: &Grant,
        approval: &Approval,
        request: &Request,
        evidence: Option<&Evidence>,
        now: i64,
    ) -> Result<Attempt, Denial> {
        readiness(policy, grant, approval, request, evidence, now)?;
        if let Some(attempt) = self.attempts.get_mut(&request.idempotency_key) {
            if attempt.request != *request
                || attempt.grant != *grant
                || attempt.approval != *approval
            {
                return Err(Denial::IdempotencyConflict);
            }
            attempt.outcome = self
                .provider_results
                .get(&request.idempotency_key)
                .copied()
                .unwrap_or(Outcome::Unknown);
            attempt.last_observed_at = now;
            return Ok(attempt.clone());
        }
        if self.attempts.values().any(|attempt| attempt.approval.id == approval.id) {
            return Err(Denial::ApprovalReplay);
        }
        // The original evidence and intent are retained before a fake provider can accept anything.
        let mut attempt = Attempt {
            request: request.clone(),
            grant: grant.clone(),
            approval: approval.clone(),
            original_evidence: evidence.ok_or(Denial::MissingEvidence)?.clone(),
            attempted_at: now,
            last_observed_at: now,
            outcome: Outcome::Attempted,
        };
        self.attempts.insert(request.idempotency_key.clone(), attempt.clone());
        self.submissions += 1;
        attempt.outcome = match std::mem::take(&mut self.next_response) {
            FakeResponse::Accept => {
                self.provider_results.insert(request.idempotency_key.clone(), Outcome::Accepted);
                Outcome::Accepted
            }
            FakeResponse::LoseResponseAfterAccept => {
                self.provider_results.insert(request.idempotency_key.clone(), Outcome::Accepted);
                Outcome::Unknown
            }
            FakeResponse::LoseResponseBeforeAccept => Outcome::Unknown,
            FakeResponse::Reject => {
                self.provider_results.insert(request.idempotency_key.clone(), Outcome::Rejected);
                Outcome::Rejected
            }
        };
        self.attempts.insert(request.idempotency_key.clone(), attempt.clone());
        Ok(attempt)
    }
}
