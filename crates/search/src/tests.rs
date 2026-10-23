use super::*;
use crate::rank::osa;

fn doc(id: u64, kind: DocKind, title: &str) -> SearchDoc {
    SearchDoc {
        id,
        kind,
        title: title.to_owned(),
        subtitle: String::new(),
    }
}

fn wi(id: u64, title: &str) -> SearchDoc {
    doc(id, DocKind::WorkItem, title)
}

fn titles(index: &Index, q: &str) -> Vec<String> {
    index
        .query(q, 50)
        .iter()
        .map(|h| index.docs()[h.doc as usize].title.clone())
        .collect()
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

/// Spec 0003 AC-3b fixture.
fn login_fixture() -> Index {
    Index::new(vec![
        wi(1, "Lagoon theme"),
        wi(2, "Fix logon redirect"),
        wi(3, "Fix logic redirect"),
        wi(4, "Fix login redirect"),
    ])
}

#[test]
fn ac3b_transposition_is_one_edit() {
    let index = login_fixture();
    assert_eq!(titles(&index, "lgoin")[0], "Fix login redirect");
    assert_eq!(titles(&index, "lgoin redirect")[0], "Fix login redirect");
    // Two edits from "login" at length 5 (budget 1): no typo match at all.
    assert!(titles(&index, "lgion").is_empty());
}

#[test]
fn ac3_typo_budget_and_numeric_tokens() {
    let index = Index::new(vec![
        wi(10, "Login page polish"),
        wi(11, "Review login flow"),
        wi(12, "Rex migration"),
        wi(4812, "Bug 4812: Cache misses"),
    ]);
    let top3 = titles(&index, "revew login");
    assert!(
        top3.iter().take(3).any(|t| t == "Review login flow"),
        "{top3:?}"
    );
    // 3 chars: zero typos.
    assert!(!titles(&index, "rev").contains(&"Rex migration".to_owned()));
    // Numbers never typo-match (neither the id nor the title).
    assert!(titles(&index, "4821").is_empty());
    assert_eq!(titles(&index, "481"), ["Bug 4812: Cache misses"]);
}

#[test]
fn ac4_exact_id_ranks_first() {
    let mut docs: Vec<SearchDoc> = (0..200)
        .map(|i| wi(48_000 + i, &format!("Mentions 48213 in text {i}")))
        .collect();
    docs.push(wi(48_213, "Unrelated title"));
    docs.push(doc(
        48_213,
        DocKind::Person,
        "People ids are not searchable",
    ));
    let index = Index::new(docs);
    let hits = index.query("#48213", 10);
    assert_eq!(index.docs()[hits[0].doc as usize].title, "Unrelated title");
    assert!(
        hits.iter()
            .all(|h| index.docs()[h.doc as usize].kind == DocKind::WorkItem)
    );
}

#[test]
fn tiers_rank_exact_prefix_boundary_substring_fuzzy_typo() {
    // Reverse order in the corpus so stable tie-breaking can't fake the result.
    let index = Index::new(vec![
        wi(1, "Logn handler"),       // typo (1 edit: missing i)
        wi(2, "Lxoxgxixn wrapper"),  // anchored subsequence
        wi(3, "Autologins cleanup"), // substring
        wi(4, "SsoLoginWidget"),     // camelCase boundary
        wi(5, "Loginflow"),          // prefix
        wi(6, "Login"),              // exact
    ]);
    assert_eq!(
        titles(&index, "login"),
        [
            "Login",
            "Loginflow",
            "SsoLoginWidget",
            "Autologins cleanup",
            "Lxoxgxixn wrapper",
            "Logn handler",
        ]
    );
}

#[test]
fn equal_scores_keep_corpus_order() {
    let index = Index::new(vec![
        wi(1, "Deploy alpha"),
        wi(2, "Deploy beta"),
        wi(3, "Deploy gamma"),
    ]);
    let hits = index.query("deploy", 10);
    assert_eq!(hits.iter().map(|h| h.doc).collect::<Vec<_>>(), [0, 1, 2]);
    assert!(hits.windows(2).all(|w| w[0].score == w[1].score));
}

#[test]
fn title_beats_subtitle_and_all_tokens_are_required() {
    let index = Index::new(vec![
        SearchDoc {
            id: 1,
            kind: DocKind::PullRequest,
            title: "Bump dependencies".into(),
            subtitle: "!1 · payments-api · Zoë Müller".into(),
        },
        SearchDoc {
            id: 2,
            kind: DocKind::Person,
            title: "Zoë Müller".into(),
            subtitle: "zoe.muller@contoso.com".into(),
        },
    ]);
    // Diacritic- and case-insensitive; the title match ranks first.
    assert_eq!(
        titles(&index, "zoe muller"),
        ["Zoë Müller", "Bump dependencies"]
    );
    assert_eq!(titles(&index, "zoe payments"), ["Bump dependencies"]);
}

#[test]
fn positions_are_title_char_indices() {
    let index = Index::new(vec![wi(7, "Zoë fixes the login")]);
    let hit = &index.query("login zo", 1)[0];
    assert_eq!(hit.positions, [0, 1, 14, 15, 16, 17, 18]);
    let hit = &index.query("lgoin", 1)[0];
    assert_eq!(hit.positions, [14, 15, 16, 17, 18]);
}

#[test]
fn empty_query_returns_corpus_order() {
    let index = login_fixture();
    let hits = index.query("  !# ", 2);
    assert_eq!(hits.iter().map(|h| h.doc).collect::<Vec<_>>(), [0, 1]);
    assert!(index.query("login", 0).is_empty());
}

#[test]
fn osa_counts_transposition_as_one_edit() {
    assert_eq!(osa(&chars("lgoin"), &chars("login"), 1), Some((1, 5, true)));
    assert_eq!(osa(&chars("lgion"), &chars("login"), 1), None);
    // Prefix of a longer word, as while typing.
    assert_eq!(
        osa(&chars("pipleine"), &chars("pipelines"), 2),
        Some((1, 8, false))
    );
    assert_eq!(osa(&chars("ab"), &chars("ba"), 1), Some((1, 2, true)));
    assert_eq!(
        osa(&chars("revew"), &chars("review"), 1),
        Some((1, 6, true))
    );
    // Two substitutions ("sittin" prefix) exceed a budget of 1.
    assert_eq!(osa(&chars("kitten"), &chars("sitting"), 1), None);
    assert_eq!(
        osa(&chars("kitten"), &chars("sitting"), 2),
        Some((2, 6, false))
    );
}

#[test]
fn typo_budget_scales_with_token_length() {
    let index = Index::new(vec![wi(1, "Deployment"), wi(2, "Cache")]);
    // 4-7 chars: one edit.
    assert_eq!(titles(&index, "cahce"), ["Cache"]);
    assert!(titles(&index, "ahcce").is_empty());
    // 8+ chars: two edits.
    assert_eq!(titles(&index, "depolymnet"), ["Deployment"]);
    assert!(titles(&index, "dpeolymnte").is_empty());
}

#[test]
fn synthetic_transposition_finds_its_target_in_a_large_corpus() {
    // Big enough for the parallel path and for many competing candidates.
    let mut docs = synthetic::corpus(PARALLEL_MIN_DOCS + 5_000, 7);
    docs.push(wi(999_999, "Quarantine flaky xylophone suite"));
    let index = Index::new(docs);
    let top = titles(&index, "xylopohne");
    assert_eq!(
        top.first().map(String::as_str),
        Some("Quarantine flaky xylophone suite")
    );
    let top = titles(&index, "xylopohne flaky");
    assert_eq!(
        top.first().map(String::as_str),
        Some("Quarantine flaky xylophone suite")
    );
}

#[test]
fn synthetic_corpus_is_deterministic_and_shaped() {
    let a = synthetic::corpus(10_000, 42);
    assert_eq!(a, synthetic::corpus(10_000, 42));
    assert_ne!(a, synthetic::corpus(10_000, 43));
    let count = |k| a.iter().filter(|d| d.kind == k).count();
    assert_eq!(count(DocKind::WorkItem), 6_000);
    assert_eq!(count(DocKind::PullRequest), 2_000);
    assert_eq!(count(DocKind::Pipeline), 500);
    assert_eq!(count(DocKind::Person), 1_000);
    let wi = a.iter().find(|d| d.kind == DocKind::WorkItem).unwrap();
    assert!(wi.title.contains(&format!(" {}: ", wi.id)), "{}", wi.title);
}

#[test]
fn synthetic_queries_include_enough_transpositions() {
    let q = synthetic::queries(500, 1);
    assert_eq!(q.len(), 500);
    assert_eq!(q, synthetic::queries(500, 1));
    // Every 8th query is a transposed vocabulary word, so no exact vocab hit.
    let transposed = q.iter().step_by(8).count();
    assert!(transposed >= 50);
    let index = Index::new(synthetic::corpus(20_000, 1));
    let found = q
        .iter()
        .step_by(8)
        .filter(|q| !index.query(q, 50).is_empty())
        .count();
    assert!(
        found * 10 >= transposed * 9,
        "{found}/{transposed} transposition queries matched"
    );
}

#[test]
fn index_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Index>();
}
