use super::*;
use crate::db::open_writer;
use crate::permissions::PermissionSet;
use crate::users;
use check_stock_core::inventory_db::InventoryDb;
use check_stock_core::models::Card;
use check_stock_core::repricing::Rule;
use chrono::TimeZone;

fn db() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_writer(&InventoryDb::in_dir(dir.path())).unwrap();
    (dir, conn)
}

fn t(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 16, 12, minute, 0).unwrap()
}

fn add_user(conn: &Connection, name: &str) -> i64 {
    users::create(conn, name, name, "$hash", &PermissionSet::default(), t(0)).unwrap()
}

/// One rule pricing everything at the market trend, nothing else in the way.
pub(crate) fn follow_trend() -> Ruleset {
    let mut rules = Ruleset::starter();
    rules.name = "Trend".to_string();
    rules.rules = vec![Rule::new("Follow the trend")];
    rules.guardrails.stale_days = 0;
    rules.guardrails.review_pct = 1000.0;
    rules.guardrails.min_change_pct = 0.0;
    rules
}

#[test]
fn the_starter_rules_until_someone_publishes() {
    let (_dir, conn) = db();
    let p = published(&conn).unwrap();
    assert_eq!(p.revision, 0);
    assert_eq!(p.ruleset, Ruleset::starter());
    assert_eq!(p.updated_by, None);
}

#[test]
fn publishing_checks_the_revision() {
    let (_dir, conn) = db();
    let rules = follow_trend();
    assert_eq!(
        publish_in_tx(&conn, &rules, 0, t(1), "Anna").unwrap(),
        Ok(1)
    );
    let p = published(&conn).unwrap();
    assert_eq!((p.revision, p.ruleset.name.as_str()), (1, "Trend"));
    assert_eq!(p.updated_by.as_deref(), Some("Anna"));
    assert_eq!(p.updated_at.as_deref(), Some("2026-11-16T12:01:00Z"));
    // Someone who started from revision 0 does not overwrite revision 1.
    let conflict = publish_in_tx(&conn, &Ruleset::starter(), 0, t(2), "Ben")
        .unwrap()
        .unwrap_err();
    assert_eq!(conflict.revision, 1);
    assert_eq!(conflict.updated_by.as_deref(), Some("Anna"));
    assert_eq!(published(&conn).unwrap().ruleset.name, "Trend");
    assert_eq!(
        publish_in_tx(&conn, &Ruleset::starter(), 1, t(3), "Ben").unwrap(),
        Ok(2)
    );
}

#[test]
fn stored_rulesets_are_sanitized() {
    let (_dir, conn) = db();
    let user = add_user(&conn, "anna");
    let mut rules = follow_trend();
    rules.rules[0].action.percent = 1e12;
    rules.guardrails.floors.common = -3.0;
    rules.name = "n".repeat(500);
    let saved = save_draft(&conn, user, 0, &rules, t(1)).unwrap();
    assert_eq!(saved.ruleset.rules[0].action.percent, 1000.0);
    assert_eq!(saved.ruleset.guardrails.floors.common, 0.0);
    assert_eq!(saved.ruleset.name.chars().count(), 80);
    assert_eq!(draft(&conn, user).unwrap().unwrap(), saved);
    publish_in_tx(&conn, &rules, 0, t(2), "Anna")
        .unwrap()
        .unwrap();
    assert_eq!(published(&conn).unwrap().ruleset, saved.ruleset);
}

#[test]
fn drafts_are_per_user() {
    let (_dir, conn) = db();
    let (anna, ben) = (add_user(&conn, "anna"), add_user(&conn, "ben"));
    save_draft(&conn, anna, 0, &follow_trend(), t(1)).unwrap();
    assert!(draft(&conn, ben).unwrap().is_none());
    assert_eq!(draft(&conn, anna).unwrap().unwrap().base_revision, 0);
    assert!(delete_draft(&conn, anna).unwrap());
    assert!(!delete_draft(&conn, anna).unwrap());
    assert!(draft(&conn, anna).unwrap().is_none());
}

#[test]
fn ruleset_files_name_the_position_never_the_content() {
    let ok = parse_ruleset_file(&follow_trend().to_json()).unwrap();
    assert_eq!(ok, follow_trend());
    assert_eq!(
        parse_ruleset_file(r#"{"nodes": []}"#).unwrap_err(),
        "This file is not a repricing ruleset (old node-graph files cannot be opened)"
    );
    let bad = parse_ruleset_file("{\"rules\": \"secret-text\"}").unwrap_err();
    assert!(
        bad.starts_with("This file is not a valid repricing ruleset (line 1"),
        "{bad}"
    );
    assert!(!bad.contains("secret"), "{bad}");
    let huge = format!(
        "{{\"rules\": [], \"name\": \"{}\"}}",
        "x".repeat(MAX_RULESET_BYTES)
    );
    assert_eq!(
        parse_ruleset_file(&huge).unwrap_err(),
        "The file is too large to be a repricing ruleset"
    );
}

#[test]
fn reviews_belong_to_one_user_and_one_import() {
    let (_dir, conn) = db();
    let (anna, ben) = (add_user(&conn, "anna"), add_user(&conn, "ben"));
    let piles = ["a".to_string(), "b".to_string()];
    set_ticks(&conn, anna, 3, &piles, Some(false)).unwrap();
    set_manual(&conn, anna, 3, "b", Some(250)).unwrap();
    let r = review(&conn, anna, 3).unwrap();
    assert_eq!(
        r.ticks,
        HashMap::from([("a".into(), false), ("b".into(), false)])
    );
    assert_eq!(r.manual, HashMap::from([("b".into(), 2.5)]));
    assert_eq!(review(&conn, ben, 3).unwrap(), Review::default());

    // Back to the default tick: the row of `a` goes, `b` keeps its price.
    set_ticks(&conn, anna, 3, &piles[..1], None).unwrap();
    set_ticks(&conn, anna, 3, &piles[1..], None).unwrap();
    let r = review(&conn, anna, 3).unwrap();
    assert!(r.ticks.is_empty());
    assert_eq!(r.manual.len(), 1);
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM srv_repricing_review", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(rows, 1);

    // A new import: the old review is ignored and counted, then dropped by
    // the next write.
    set_ticks(&conn, anna, 3, &piles[..1], Some(true)).unwrap();
    let r = review(&conn, anna, 4).unwrap();
    assert_eq!((r.dropped_ticks, r.dropped_manual), (1, 1));
    assert!(r.ticks.is_empty() && r.manual.is_empty());
    set_manual(&conn, anna, 4, "c", Some(100)).unwrap();
    let r = review(&conn, anna, 4).unwrap();
    assert_eq!((r.dropped_ticks, r.dropped_manual), (0, 0));
    assert_eq!(r.manual, HashMap::from([("c".into(), 1.0)]));
    assert_eq!(clear_review(&conn, anna).unwrap(), 1);
    assert_eq!(review(&conn, anna, 4).unwrap(), Review::default());
}

fn new_export<'a>(user: i64, csv: &'a str) -> NewExport<'a> {
    NewExport {
        user_id: user,
        username: "anna",
        revision: 1,
        version: 3,
        changes: 2,
        raises: 1,
        lowers: 1,
        file_name: "repricing_2026-11-16.csv",
        csv,
        undo_csv: "undo",
    }
}

#[test]
fn the_last_twenty_exports_are_kept() {
    let (_dir, conn) = db();
    let anna = add_user(&conn, "anna");
    let first = keep_export(&conn, t(0), &new_export(anna, "first")).unwrap();
    assert_eq!(
        export_file(&conn, first, false).unwrap(),
        Some(("repricing_2026-11-16.csv".into(), b"first".to_vec()))
    );
    assert_eq!(
        export_file(&conn, first, true).unwrap(),
        Some(("repricing_2026-11-16_undo.csv".into(), b"undo".to_vec()))
    );
    for i in 1..=KEEP_EXPORTS {
        keep_export(&conn, t(i as u32), &new_export(anna, "later")).unwrap();
    }
    let kept = exports(&conn).unwrap();
    assert_eq!(kept.len() as i64, KEEP_EXPORTS);
    assert_eq!(kept[0].id, first + KEEP_EXPORTS, "newest first");
    assert_eq!(kept[0].undo_file_name, "repricing_2026-11-16_undo.csv");
    assert_eq!(export_file(&conn, first, false).unwrap(), None);
    // Deleting the user keeps the export (name kept as text).
    conn.execute("DELETE FROM srv_users WHERE id = ?1", [anna])
        .unwrap();
    assert_eq!(exports(&conn).unwrap()[0].username, "anna");
}

// ── Prepared stock and review rows ─────────────────────────────────────────

fn card(id: u64, name: &str, qty: &str, price: &str, location: &str) -> Card {
    Card {
        cardmarket_id: id.to_string(),
        name: name.to_string(),
        quantity: qty.to_string(),
        price: price.to_string(),
        set: "Test Set".to_string(),
        set_code: "TST".to_string(),
        rarity: "Common".to_string(),
        location: Some(location.to_string()),
        ..Card::test_default()
    }
}

fn market_row(id: u64, trend: f64) -> LatestPrice {
    LatestPrice {
        id_product: id,
        price_date: "2026-11-15".to_string(),
        avg: None,
        low: None,
        trend: Some(trend),
        avg1: None,
        avg7: None,
        avg30: None,
        avg_foil: None,
        low_foil: None,
        trend_foil: None,
        avg1_foil: None,
        avg7_foil: None,
        avg30_foil: None,
    }
}

fn market(rows: &[(u64, f64)]) -> Arc<MarketData> {
    Arc::new(MarketData {
        cards: Vec::new(),
        latest: rows
            .iter()
            .map(|&(id, t)| (id, market_row(id, t)))
            .collect(),
        snapshots: Default::default(),
        today: NaiveDate::from_ymd_opt(2026, 11, 16).unwrap(),
        loaded_at: t(0),
        version: 7,
        products: rows.len(),
    })
}

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 16).unwrap()
}

/// Alpha (raise 1 → 2), Beta (drop 4 → 3, reserved for a bulk lot), Gamma
/// (no market), Delta sold out (left out), Epsilon (big raise 1 → 10).
pub(crate) fn prepared() -> Prepared {
    let cards = vec![
        card(1, "Alpha", "2", "1.00", "A-0-0-1-L1-R"),
        card(2, "Beta", "1", "4.00", "A-0-0-2-L1-R"),
        card(3, "Gamma", "1", "1.00", "A-0-0-3-L1-R"),
        card(4, "Delta", "0", "1.00", "A-0-0-4-L1-R"),
        card(5, "Epsilon", "1", "1.00", "A-0-0-5-L1-R"),
    ];
    let reserved = HashSet::from([card_variant_key(&cards[1], CATEGORY)]);
    let alpha_key = card_variant_key(&cards[0], CATEGORY);
    let dates = HashMap::from([(alpha_key.clone(), "2026-01-01".to_string())]);
    let sales = HashMap::from([(
        alpha_key,
        vec![("2026-11-01".to_string(), 1), ("2026-11-10".to_string(), 2)],
    )]);
    let source = Arc::new(Source::new(3, 0, "f.csv", "2026-11-16T10:00:00Z", cards));
    Prepared::build(
        12,
        day(),
        source,
        Some(market(&[(1, 2.0), (2, 3.0), (4, 9.0), (5, 10.0)])),
        &dates,
        sales,
        &reserved,
    )
}

fn evaluated(p: &Prepared, rules: &Ruleset) -> Evaluation {
    evaluate(
        rules,
        &p.listings,
        p.market_rows(),
        &HashMap::new(),
        p.today,
    )
}

#[test]
fn listings_carry_piles_facts_and_reservations() {
    let p = prepared();
    let names: Vec<&str> = p.listings.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        ["Alpha", "Beta", "Gamma", "Epsilon"],
        "sold out left out"
    );
    assert_eq!(p.listings[3].row, 4, "row = index in the source");
    assert_eq!(p.piles[3], p.source.ids[4]);
    assert_eq!(p.listing_of(&p.source.ids[4]), Some(3));
    assert_eq!(p.listing_of("nope"), None);
    assert_eq!(p.held_back, [false, true, false, false]);
    assert_eq!(p.listings[0].facts.age_days, Some(319));
    assert_eq!(p.listings[0].facts.sold_30d, 3);
    assert_eq!(
        p.sales_of(0),
        [("2026-11-10".to_string(), 2), ("2026-11-01".to_string(), 1)]
    );
    assert!(p.sales_of(1).is_empty());
    assert_eq!(p.rarities, ["Common", "Uncommon", "Rare", "Mythic"]);
    assert_eq!(p.market_version(), Some(7));
}

#[test]
fn held_back_listings_are_never_ticked() {
    let p = prepared();
    let mut rules = follow_trend();
    rules.guardrails.review_pct = 100.0;
    rules.guardrails.review_min_eur = 1.0;
    let eval = evaluated(&p, &rules);
    let outcomes: Vec<Outcome> = eval.proposals.iter().map(|x| x.outcome).collect();
    assert_eq!(
        outcomes,
        [
            Outcome::Raise,
            Outcome::Lower,
            Outcome::NoMarketData,
            Outcome::Raise
        ]
    );
    assert!(eval.proposals[3].review, "1 → 10 is a big raise");
    let ticks = HashMap::from([(p.piles[1].clone(), true)]);
    let view = ReviewView {
        prepared: &p,
        eval: &eval,
        ticks: &ticks,
    };
    // Alpha ticked by default; Beta ticked by hand but held back; Epsilon
    // waits for review.
    assert_eq!(
        (0..4).map(|i| view.ticked(i)).collect::<Vec<_>>(),
        [true, false, false, false]
    );
    assert_eq!(view.overridden(1), Some(true));
    let (summary, held) = view.summary();
    assert_eq!((summary.selected, summary.review, held), (1, 1, 1));
    assert_eq!(summary.value_after - summary.value_before, 2.0);
    assert_eq!(
        view.piles_of(&[0, 1, 2, 3]),
        [p.piles[0].clone(), p.piles[3].clone()]
    );
}

#[test]
fn rows_filter_search_and_sort_like_the_desktop() {
    let p = prepared();
    let eval = evaluated(&p, &follow_trend());
    let ticks = HashMap::new();
    let view = ReviewView {
        prepared: &p,
        eval: &eval,
        ticks: &ticks,
    };
    let rows = |f, rule, search, sort, desc| view.rows(f, rule, search, sort, desc);
    use ReviewFilter as F;
    use ReviewSort as S;
    // Impact = |change| × copies: Epsilon 9, Alpha 2, Beta 1.
    assert_eq!(rows(F::Changes, None, "", S::Impact, true), [3, 0, 1]);
    assert_eq!(rows(F::Changes, None, "", S::Impact, false), [1, 0, 3]);
    assert_eq!(rows(F::Raises, None, "", S::Name, false), [0, 3]);
    assert_eq!(rows(F::Lowers, None, "", S::Name, false), [1]);
    assert_eq!(rows(F::Unchanged, None, "", S::Name, false), [2]);
    assert_eq!(rows(F::HeldBack, None, "", S::Name, false), [1]);
    assert_eq!(rows(F::All, None, "", S::Name, true), [2, 3, 1, 0]);
    assert_eq!(rows(F::All, None, "alp, gam", S::Name, false), [0, 2]);
    assert_eq!(
        rows(F::All, None, "tst", S::Name, false),
        [0, 1, 3, 2],
        "set code"
    );
    assert_eq!(
        rows(F::All, None, "ts", S::Name, false),
        Vec::<usize>::new()
    );
    assert_eq!(rows(F::All, None, "test set", S::Name, false).len(), 4);
    assert_eq!(rows(F::All, Some(0), "", S::Name, false), [0, 1, 3, 2]);
    assert_eq!(
        rows(F::All, Some(1), "", S::Name, false),
        Vec::<usize>::new()
    );
    // Ties by name: every listing has one copy except Alpha.
    assert_eq!(rows(F::All, None, "", S::Quantity, false), [1, 3, 2, 0]);
}

#[test]
fn needs_review_lists_big_changes_not_ticked_yet() {
    let p = prepared();
    let mut rules = follow_trend();
    rules.guardrails.review_pct = 100.0;
    let eval = evaluated(&p, &rules);
    let none = HashMap::new();
    let view = ReviewView {
        prepared: &p,
        eval: &eval,
        ticks: &none,
    };
    assert_eq!(
        view.rows(ReviewFilter::NeedsReview, None, "", ReviewSort::Name, false),
        [3]
    );
    let ticked = HashMap::from([(p.piles[3].clone(), true)]);
    let view = ReviewView {
        prepared: &p,
        eval: &eval,
        ticks: &ticked,
    };
    assert!(view
        .rows(ReviewFilter::NeedsReview, None, "", ReviewSort::Name, false)
        .is_empty());
}

#[test]
fn hand_prices_map_to_listings() {
    let p = prepared();
    let review = Review {
        manual: HashMap::from([(p.piles[2].clone(), 7.0), ("gone".into(), 1.0)]),
        ..Review::default()
    };
    assert_eq!(p.manual_by_listing(&review), HashMap::from([(2, 7.0)]));
}

#[test]
fn evaluations_are_cached_by_all_their_inputs() {
    let p = prepared();
    let rules = follow_trend();
    let base = eval_key(&p, &rules, &HashMap::new());
    assert_eq!(base, eval_key(&p, &rules, &HashMap::new()));
    let mut other = rules.clone();
    other.rules[0].action.percent = 99.0;
    assert_ne!(base, eval_key(&p, &other, &HashMap::new()));
    assert_ne!(base, eval_key(&p, &rules, &HashMap::from([(0, 1.0)])));
    assert_ne!(
        eval_key(&p, &rules, &HashMap::from([(0, 1.0)])),
        eval_key(&p, &rules, &HashMap::from([(0, 2.0)]))
    );
    let mut q = prepared();
    q.data_version += 1;
    assert_ne!(base, eval_key(&q, &rules, &HashMap::new()));
    let mut q = prepared();
    q.market = None;
    assert_ne!(base, eval_key(&q, &rules, &HashMap::new()));
    let mut q = prepared();
    q.today = q.today.succ_opt().unwrap();
    assert_ne!(base, eval_key(&q, &rules, &HashMap::new()));
}

#[tokio::test]
async fn the_service_reuses_one_evaluation() {
    let service = RepricingService::default();
    let p = Arc::new(prepared());
    let rules = follow_trend();
    let a = service.evaluate(&p, &rules, HashMap::new()).await.unwrap();
    let b = service.evaluate(&p, &rules, HashMap::new()).await.unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    let c = service
        .evaluate(&p, &rules, HashMap::from([(2, 5.0)]))
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&a, &c));
    assert!(c.proposals[2].manual);
    // Old evaluations make room for new ones.
    for pct in 0..EVAL_CACHE {
        let mut r = rules.clone();
        r.rules[0].action.percent = 50.0 + pct as f64;
        service.evaluate(&p, &r, HashMap::new()).await.unwrap();
    }
    let d = service.evaluate(&p, &rules, HashMap::new()).await.unwrap();
    assert!(!Arc::ptr_eq(&a, &d));
    assert_eq!(*a, *d);
}
