//! RFC 0032 — live verification against the real Snapshot hub.
//!
//! `#[ignore]`d so CI and `cargo test --workspace` stay offline and deterministic; run with
//! `cargo test -p ekos-plugin-governance --test live_snapshot -- --ignored --nocapture`.
//! The hub is public and unauthenticated, so no API key is needed.
//!
//! These exist because the unit tests only ever saw the documented response shapes. What they
//! pinned down on 2026-09-22 (`devlog_194`):
//!
//! - every field `parse_proposal` reads is present on a real node, with the types it assumes
//!   (`created`/`end` integer unix seconds, `scores` an array of numbers);
//! - paging behaves as the client assumes on a space with more than one page;
//! - **a wrong space id is not an error.** A renamed space (`aave.eth`, now `aavedao.eth`), a
//!   misspelled one and an empty string all return HTTP 200 with `{"data":{"proposals":[]}}` and
//!   no GraphQL `errors` key. `SnapshotObserver` warns on an empty result because of this.

use ekos_plugin_governance::{DEFAULT_HUB_URL, GovernanceClient, SnapshotClient};

/// A space that fits in a single short page — the client must read a short page as the end.
const SMALL_SPACE: &str = "ens.eth";
/// A space with more than 100 proposals — exercises the paging loop for real.
const PAGED_SPACE: &str = "uniswapgovernance.eth";

fn client() -> SnapshotClient {
    SnapshotClient::new(DEFAULT_HUB_URL)
}

#[tokio::test]
#[ignore = "live: hits hub.snapshot.org"]
async fn a_real_proposal_carries_every_field_the_parser_reads() {
    let proposals = client().list_proposals(SMALL_SPACE).await.unwrap();
    assert!(!proposals.is_empty(), "{SMALL_SPACE} should have proposals");

    let p = proposals
        .iter()
        .find(|p| p.state == "closed")
        .expect("at least one closed proposal");
    assert!(!p.id.is_empty(), "id keys the artifact");
    assert!(!p.title.is_empty());
    assert!(!p.author.is_empty());
    assert!(!p.choices.is_empty(), "a closed proposal has choices");
    assert_eq!(
        p.choices.len(),
        p.scores.len(),
        "derive_outcome requires choices and scores to line up"
    );
    assert!(
        p.created.timestamp() > 0 && p.end >= p.created,
        "created/end parsed as real unix seconds: {} -> {}",
        p.created,
        p.end
    );
}

#[tokio::test]
#[ignore = "live: hits hub.snapshot.org"]
async fn paging_crosses_a_page_boundary_and_returns_distinct_proposals() {
    let proposals = client().list_proposals(PAGED_SPACE).await.unwrap();
    assert!(
        proposals.len() > 100,
        "{PAGED_SPACE} should span more than one 100-row page, got {}",
        proposals.len()
    );

    let mut ids: Vec<&str> = proposals.iter().map(|p| p.id.as_str()).collect();
    ids.sort_unstable();
    let total = ids.len();
    ids.dedup();
    assert_eq!(total, ids.len(), "paging must not repeat a proposal");

    let created: Vec<_> = proposals.iter().map(|p| p.created).collect();
    assert!(
        created.windows(2).all(|w| w[0] <= w[1]),
        "the query asks for created ASC; ordering must survive paging"
    );
}

/// The finding behind `SnapshotObserver`'s empty-result warning: a wrong space id is reported as
/// *no proposals*, never as an error, so nothing downstream can tell the two apart on its own.
#[tokio::test]
#[ignore = "live: hits hub.snapshot.org"]
async fn a_wrong_space_id_returns_an_empty_list_not_an_error() {
    for space in [
        "aave.eth",                            // real DAO; space since renamed to aavedao.eth
        "totally-not-a-real-space-xyz123.eth", // never existed
        "",                                    // unset/empty env var
    ] {
        let proposals = client()
            .list_proposals(space)
            .await
            .unwrap_or_else(|e| panic!("{space:?} should not error, got: {e}"));
        assert!(
            proposals.is_empty(),
            "{space:?} unexpectedly returned {} proposals",
            proposals.len()
        );
    }

    // ...and the space it was renamed to does work, so the empty results above are about the id,
    // not about the hub being down or the query being malformed.
    assert!(
        !client()
            .list_proposals("aavedao.eth")
            .await
            .unwrap()
            .is_empty(),
        "aavedao.eth should have proposals"
    );
}
