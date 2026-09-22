//! DAO governance proposal observer (RFC 0032) — Snapshot.
//!
//! Observes the proposals of one Snapshot space through Snapshot's public GraphQL hub (no API key)
//! and emits one `ObservationArtifact` per proposal. Snapshot was chosen over Discourse for v1
//! (RFC 0032 Open Question 2): its proposals carry a structured outcome — choices, per-choice
//! scores, a close time — so "approved, and when" is data rather than something scraped from forum
//! prose. The amount and recipient a proposal approves are still free text in its body; extracting
//! them is `GovernanceAnalyzerPass`'s job and is honestly best-effort.
//!
//! **Outcome** is derived here, once, from the structured fields: a closed proposal whose winning
//! choice reads as affirmative ("For", "Yes", "Approve" …) is `approved`; a winning choice that
//! reads as negative is `rejected`; anything else — an unfamiliar choice label, a tie, a proposal
//! still open — is `unknown`. `unknown` is never promoted to `approved`.
//!
//! **Status of the real client:** written to Snapshot's documented GraphQL schema and parsed by
//! unit-tested pure functions, but **not run against the live hub**. Live behaviour (rate limits,
//! very large spaces) is unverified.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Proposal {
    pub id: String,
    pub title: String,
    pub body: String,
    /// Snapshot state: `pending` / `active` / `closed`.
    pub state: String,
    pub author: String,
    pub choices: Vec<String>,
    pub scores: Vec<f64>,
    pub created: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Approved,
    Rejected,
    Unknown,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Approved => "approved",
            Outcome::Rejected => "rejected",
            Outcome::Unknown => "unknown",
        }
    }
}

const AFFIRMATIVE: &[&str] = &[
    "for",
    "yes",
    "yay",
    "approve",
    "approved",
    "accept",
    "in favor",
    "in favour",
];
const NEGATIVE: &[&str] = &[
    "against", "no", "nay", "reject", "rejected", "deny", "oppose",
];

/// The outcome of a proposal, from its structured fields only. `Unknown` unless the proposal is
/// closed and has a single, clearly winning choice with a recognisable label.
pub fn derive_outcome(p: &Proposal) -> Outcome {
    if p.state != "closed" || p.choices.is_empty() || p.choices.len() != p.scores.len() {
        return Outcome::Unknown;
    }
    let max = p.scores.iter().cloned().fold(f64::MIN, f64::max);
    if max <= 0.0 {
        return Outcome::Unknown;
    }
    let winners: Vec<usize> = (0..p.scores.len())
        .filter(|&i| p.scores[i] == max)
        .collect();
    if winners.len() != 1 {
        return Outcome::Unknown; // a tie decides nothing
    }
    let label = p.choices[winners[0]].trim().to_ascii_lowercase();
    if AFFIRMATIVE
        .iter()
        .any(|a| label == *a || label.starts_with(&format!("{a} ")))
    {
        Outcome::Approved
    } else if NEGATIVE
        .iter()
        .any(|a| label == *a || label.starts_with(&format!("{a} ")))
    {
        Outcome::Rejected
    } else {
        Outcome::Unknown
    }
}

#[derive(Debug, Error)]
pub enum GovernanceClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("snapshot api error: {0}")]
    Api(String),
}

#[async_trait]
pub trait GovernanceClient: Send + Sync {
    /// Every proposal of `space`, oldest first.
    async fn list_proposals(&self, space: &str) -> Result<Vec<Proposal>, GovernanceClientError>;
}

/// Parse one Snapshot proposal node. `None` when it has no id (cannot be keyed).
pub fn parse_proposal(v: &Value) -> Option<Proposal> {
    let ts = |k: &str| {
        Utc.timestamp_opt(v[k].as_i64().unwrap_or_default(), 0)
            .single()
            .unwrap_or_default()
    };
    Some(Proposal {
        id: v["id"].as_str()?.to_string(),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        body: v["body"].as_str().unwrap_or_default().to_string(),
        state: v["state"].as_str().unwrap_or_default().to_string(),
        author: v["author"].as_str().unwrap_or_default().to_string(),
        choices: v["choices"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        scores: v["scores"]
            .as_array()
            .map(|a| a.iter().map(|s| s.as_f64().unwrap_or_default()).collect())
            .unwrap_or_default(),
        created: ts("created"),
        end: ts("end"),
    })
}

/// Unwrap `{"data":{"proposals":[…]}}`, surfacing GraphQL `errors` instead of an empty list.
pub fn parse_proposals_response(body: &Value) -> Result<Vec<Proposal>, GovernanceClientError> {
    if let Some(errors) = body.get("errors") {
        return Err(GovernanceClientError::Api(errors.to_string()));
    }
    let rows = body["data"]["proposals"].as_array().ok_or_else(|| {
        GovernanceClientError::Api(format!("no data.proposals in response: {body}"))
    })?;
    Ok(rows.iter().filter_map(parse_proposal).collect())
}

pub const DEFAULT_HUB_URL: &str = "https://hub.snapshot.org/graphql";
const PAGE: usize = 100;
const QUERY: &str = "query($space:String!,$first:Int!,$skip:Int!){proposals(first:$first,skip:$skip,\
where:{space:$space},orderBy:\"created\",orderDirection:asc){id title body choices scores state author created end}}";

pub struct SnapshotClient {
    hub_url: String,
    max_pages: u32,
    http: reqwest::Client,
}

impl SnapshotClient {
    pub fn new(hub_url: impl Into<String>) -> Self {
        Self {
            hub_url: hub_url.into(),
            max_pages: 50,
            http: reqwest::Client::new(),
        }
    }

    /// Bound the crawl: at most `max_pages` × 100 proposals — never an unbounded history walk.
    pub fn with_max_pages(mut self, max_pages: u32) -> Self {
        self.max_pages = max_pages.max(1);
        self
    }
}

#[async_trait]
impl GovernanceClient for SnapshotClient {
    async fn list_proposals(&self, space: &str) -> Result<Vec<Proposal>, GovernanceClientError> {
        let mut all = Vec::new();
        for page in 0..self.max_pages as usize {
            let resp = self
                .http
                .post(&self.hub_url)
                .json(&serde_json::json!({
                    "query": QUERY,
                    "variables": { "space": space, "first": PAGE, "skip": page * PAGE },
                }))
                .send()
                .await?;
            if !resp.status().is_success() {
                return Err(GovernanceClientError::Api(format!(
                    "http {}",
                    resp.status()
                )));
            }
            let batch = parse_proposals_response(&resp.json::<Value>().await?)?;
            let done = batch.len() < PAGE;
            all.extend(batch);
            if done {
                break;
            }
        }
        Ok(all)
    }
}

/// In-process client for unit tests — fixed proposals, no network.
pub struct MockGovernanceClient {
    pub proposals: Vec<Proposal>,
}

impl MockGovernanceClient {
    pub fn new(proposals: Vec<Proposal>) -> Self {
        Self { proposals }
    }
}

#[async_trait]
impl GovernanceClient for MockGovernanceClient {
    async fn list_proposals(&self, _space: &str) -> Result<Vec<Proposal>, GovernanceClientError> {
        Ok(self.proposals.clone())
    }
}

/// Emits one artifact per proposal.
pub struct SnapshotObserver {
    client: Arc<dyn GovernanceClient>,
    space: String,
}

impl SnapshotObserver {
    pub fn new(client: Arc<dyn GovernanceClient>, space: impl Into<String>) -> Self {
        Self {
            client,
            space: space.into(),
        }
    }
}

#[async_trait]
impl Observer for SnapshotObserver {
    fn name(&self) -> &str {
        "governance"
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let proposals = self.client.list_proposals(&self.space).await.map_err(|e| {
            ObserveError::connector(format!("governance list_proposals failed: {e}"))
        })?;
        // RFC 0032 — verified live against hub.snapshot.org 2026-09-22: a renamed space
        // (`aave.eth` → `aavedao.eth`), a misspelled one and an empty string all return HTTP 200
        // with `{"data":{"proposals":[]}}` and no GraphQL `errors` key, indistinguishable from a
        // space that genuinely has no proposals. Left silent, `ekos treasury scan` would then
        // report every payment as lacking governance approval — an actively wrong compliance
        // answer, not a missing one. So an empty result is always worth a word.
        if proposals.is_empty() {
            tracing::warn!(
                space = %self.space,
                "snapshot space returned 0 proposals — if that is unexpected, check the space id \
                 (a renamed or misspelled space returns an empty list with HTTP 200, not an error)"
            );
        }
        let mut pkg = ObservationPackage::new("governance", format!("snapshot:{}", self.space));
        for p in &proposals {
            let outcome = derive_outcome(p);
            let data = serde_json::json!({
                "platform": "snapshot",
                "space": self.space,
                "proposal_id": p.id,
                "title": p.title,
                "body": p.body,
                "state": p.state,
                "author": p.author,
                "choices": p.choices,
                "scores": p.scores,
                "outcome": outcome.as_str(),
                "created": p.created.to_rfc3339(),
                "end": p.end.to_rfc3339(),
            });
            let target = format!("snapshot:{}:{}", self.space, p.id);
            pkg.push(
                ObservationArtifact::new("governance", &target, data)
                    .with_producer("ekos-plugin-governance"),
            );
        }
        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn proposal(state: &str, choices: &[&str], scores: &[f64]) -> Proposal {
        Proposal {
            id: "0xprop".into(),
            title: "Fund the marketing team".into(),
            body: "Pay 50,000 USDC to 0x1111111111111111111111111111111111111111".into(),
            state: state.into(),
            author: "0xauthor".into(),
            choices: choices.iter().map(|c| c.to_string()).collect(),
            scores: scores.to_vec(),
            created: Utc.timestamp_opt(1_770_000_000, 0).unwrap(),
            end: Utc.timestamp_opt(1_770_500_000, 0).unwrap(),
        }
    }

    #[test]
    fn outcome_is_approved_only_for_a_closed_clear_affirmative_winner() {
        assert_eq!(
            derive_outcome(&proposal(
                "closed",
                &["For", "Against", "Abstain"],
                &[900.0, 100.0, 5.0]
            )),
            Outcome::Approved
        );
        assert_eq!(
            derive_outcome(&proposal("closed", &["Yes", "No"], &[10.0, 20.0])),
            Outcome::Rejected
        );
    }

    #[test]
    fn unknown_is_never_promoted_to_approved() {
        assert_eq!(
            derive_outcome(&proposal("active", &["For", "Against"], &[10.0, 1.0])),
            Outcome::Unknown,
            "still open"
        );
        assert_eq!(
            derive_outcome(&proposal("closed", &["For", "Against"], &[5.0, 5.0])),
            Outcome::Unknown,
            "a tie decides nothing"
        );
        assert_eq!(
            derive_outcome(&proposal("closed", &["Option A", "Option B"], &[9.0, 1.0])),
            Outcome::Unknown,
            "unrecognised labels"
        );
        assert_eq!(
            derive_outcome(&proposal("closed", &["For", "Against"], &[0.0, 0.0])),
            Outcome::Unknown,
            "no votes"
        );
        assert_eq!(
            derive_outcome(&proposal("closed", &["For"], &[])),
            Outcome::Unknown,
            "mismatched shapes"
        );
    }

    #[test]
    fn parses_a_snapshot_response_and_surfaces_graphql_errors() {
        let ok = json!({"data":{"proposals":[
            {"id":"0xa","title":"T","body":"B","choices":["For","Against"],"scores":[3.0,1.0],"state":"closed","author":"0x1","created":1770000000,"end":1770500000},
            {"title":"no id, cannot be keyed"}
        ]}});
        let ps = parse_proposals_response(&ok).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].choices, vec!["For", "Against"]);
        assert_eq!(ps[0].end.timestamp(), 1_770_500_000);
        let err = json!({"errors":[{"message":"rate limited"}]});
        assert!(matches!(
            parse_proposals_response(&err),
            Err(GovernanceClientError::Api(_))
        ));
    }

    #[tokio::test]
    async fn emits_one_artifact_per_proposal_with_the_derived_outcome() {
        let client = Arc::new(MockGovernanceClient::new(vec![proposal(
            "closed",
            &["For", "Against"],
            &[9.0, 1.0],
        )]));
        let pkg = SnapshotObserver::new(client, "dao.eth")
            .scan(&ScanContext::new("."))
            .await
            .unwrap();
        assert_eq!(pkg.len(), 1);
        let d = &pkg.artifacts[0].content.data;
        assert_eq!(d["outcome"], "approved");
        assert_eq!(d["space"], "dao.eth");
        assert!(d["end"].as_str().unwrap().starts_with("2026-"));
    }

    #[tokio::test]
    async fn same_proposals_same_artifact_ids() {
        let mk = || {
            Arc::new(MockGovernanceClient::new(vec![proposal(
                "closed",
                &["For", "Against"],
                &[9.0, 1.0],
            )]))
        };
        let ctx = ScanContext::new(".");
        let a = SnapshotObserver::new(mk(), "dao.eth")
            .scan(&ctx)
            .await
            .unwrap();
        let b = SnapshotObserver::new(mk(), "dao.eth")
            .scan(&ctx)
            .await
            .unwrap();
        assert_eq!(a.artifacts[0].id, b.artifacts[0].id);
    }
}
