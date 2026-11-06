use super::*;

// ── Helpers ─────────────────────────────────────────────────────────────────

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 6).unwrap()
}

fn card(id: &str, name: &str, price: &str) -> Card {
    let mut c = Card::test_default();
    c.cardmarket_id = id.to_string();
    c.name = name.to_string();
    c.price = price.to_string();
    c.quantity = "1".to_string();
    c.rarity = "Rare".to_string();
    c
}

fn listing(id: &str, price: &str) -> Listing {
    Listing::from_card(
        0,
        &card(id, "Lightning Bolt", price),
        ListingFacts::default(),
    )
    .unwrap()
}

fn row(id: u64, trend: Option<f64>) -> LatestPrice {
    LatestPrice {
        id_product: id,
        price_date: "2026-11-05".to_string(),
        avg: None,
        low: None,
        trend,
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

fn market_of(rows: Vec<LatestPrice>) -> HashMap<u64, LatestPrice> {
    rows.into_iter().map(|r| (r.id_product, r)).collect()
}

/// A ruleset with one rule, no floors above Cardmarket's minimum, no dead
/// band and no condition factors — so tests see the raw arithmetic.
fn plain(action: PriceAction, conditions: Vec<Condition>) -> Ruleset {
    let mut rs = Ruleset {
        version: RULESET_VERSION,
        name: "test".to_string(),
        rules: vec![Rule {
            name: "r".to_string(),
            enabled: true,
            conditions,
            action,
        }],
        guardrails: Guardrails::default(),
        adjustments: Adjustments::default(),
    };
    rs.guardrails.floors = RarityFloors {
        common: 0.0,
        uncommon: 0.0,
        rare: 0.0,
        mythic: 0.0,
        other: 0.0,
    };
    rs.guardrails.min_change_eur = 0.0;
    rs.guardrails.min_change_pct = 0.0;
    rs.adjustments.condition_pct.clear();
    rs
}

fn trend_action() -> PriceAction {
    PriceAction {
        adjust: false,
        ..PriceAction::default()
    }
}

fn one(rs: &Ruleset, l: &Listing, market: &HashMap<u64, LatestPrice>) -> Proposal {
    evaluate(
        rs,
        std::slice::from_ref(l),
        market,
        &HashMap::new(),
        today(),
    )
    .proposals
    .remove(0)
}

// ── Listings ────────────────────────────────────────────────────────────────

#[test]
fn listings_skip_rows_without_stock() {
    let mut sold_out = card("1", "A", "1.00");
    sold_out.quantity = "0".to_string();
    let mut bad_qty = card("2", "B", "1.00");
    bad_qty.quantity = "".to_string();
    let cards = vec![sold_out, bad_qty, card("3", "C", "1.00")];
    let listings = listings_from_cards(&cards, |_| ListingFacts::default());
    assert_eq!(listings.len(), 1);
    assert_eq!(listings[0].row, 2, "row keeps the CSV index");
}

#[test]
fn listing_reads_comma_prices_foil_finish_and_canonical_condition() {
    let mut c = card("5", "Shock", "1,25");
    c.finish_type = "Foil".to_string();
    c.condition = "near_mint".to_string();
    let l = Listing::from_card(7, &c, ListingFacts::default()).unwrap();
    assert_eq!(l.price, 1.25);
    assert!(l.is_foil);
    assert_eq!(l.condition, "NM");
    assert_eq!(l.product_id, Some(5));
}

#[test]
fn unparseable_price_becomes_zero_and_is_never_repriced() {
    let l = listing("1", "n/a");
    assert_eq!(l.price, 0.0);
    let rs = plain(trend_action(), vec![]);
    let p = one(&rs, &l, &market_of(vec![row(1, Some(2.0))]));
    assert_eq!(p.outcome, Outcome::NoPrice);
}

#[test]
fn distinct_by_copies_orders_by_copies() {
    let mut a = card("1", "A", "1.00");
    a.language = "German".to_string();
    a.quantity = "5".to_string();
    let b = card("2", "B", "1.00");
    let listings = listings_from_cards(&[a, b], |_| ListingFacts::default());
    assert_eq!(
        distinct_by_copies(&listings, |l| &l.language),
        vec!["German".to_string(), "English".to_string()]
    );
}

// ── Facts ───────────────────────────────────────────────────────────────────

#[test]
fn facts_from_history() {
    let sales = vec![
        ("2026-06-01".to_string(), 1),
        ("2026-10-20".to_string(), 2),
        ("2026-11-01".to_string(), 1),
        ("garbage".to_string(), 9),
    ];
    let f = ListingFacts::from_history(Some("2026-01-01"), &sales, today());
    assert_eq!(f.age_days, Some(309));
    assert_eq!(f.last_sale, NaiveDate::from_ymd_opt(2026, 11, 1));
    assert_eq!(f.days_since_sale, Some(5));
    assert_eq!(f.sold_30d, 3, "only sales inside the 30-day window count");
}

#[test]
fn facts_without_sales_count_from_listing_date() {
    let f = ListingFacts::from_history(Some("2026-10-07"), &[], today());
    assert_eq!(f.days_since_sale, Some(30));
    assert_eq!(f.sold_30d, 0);
    let unknown = ListingFacts::from_history(None, &[], today());
    assert_eq!(unknown.age_days, None);
    assert_eq!(unknown.days_since_sale, None);
}

#[test]
fn facts_relisting_after_last_sale_resets_days_since_sale() {
    let sales = vec![("2026-01-10".to_string(), 1)];
    let f = ListingFacts::from_history(Some("2026-11-01"), &sales, today());
    assert_eq!(f.days_since_sale, Some(5));
}

// ── Conditions ──────────────────────────────────────────────────────────────

fn matches(cond: Condition, l: &Listing, market: &HashMap<u64, LatestPrice>) -> bool {
    let rs = plain(trend_action(), vec![cond]);
    one(&rs, l, market).rule.is_some()
}

#[test]
fn list_conditions_match_any_value_and_empty_matches_all() {
    let l = listing("1", "1.00");
    let m = HashMap::new();
    assert!(matches(Condition::Rarity(vec![]), &l, &m));
    assert!(matches(
        Condition::Rarity(vec!["rare".into(), "Mythic".into()]),
        &l,
        &m
    ));
    assert!(!matches(Condition::Rarity(vec!["Common".into()]), &l, &m));
    assert!(matches(
        Condition::CardCondition(vec!["near_mint".into()]),
        &l,
        &m
    ));
    assert!(!matches(
        Condition::CardCondition(vec!["EX".into()]),
        &l,
        &m
    ));
    assert!(matches(Condition::Language(vec!["english".into()]), &l, &m));
    assert!(!matches(Condition::Language(vec!["German".into()]), &l, &m));
}

#[test]
fn finish_condition() {
    let l = listing("1", "1.00");
    let m = HashMap::new();
    assert!(matches(Condition::Finish(Finish::NonFoil), &l, &m));
    assert!(!matches(Condition::Finish(Finish::Foil), &l, &m));
}

#[test]
fn price_range_conditions_are_inclusive() {
    let l = listing("1", "2.00");
    let m = market_of(vec![row(1, Some(5.0))]);
    let listed = |min, max| Condition::ListedPrice { min, max };
    assert!(matches(listed(Some(2.0), Some(2.0)), &l, &m));
    assert!(!matches(listed(Some(2.01), None), &l, &m));
    assert!(matches(listed(None, None), &l, &m));
    let market = |min, max| Condition::MarketPrice { min, max };
    assert!(matches(market(Some(5.0), None), &l, &m));
    assert!(!matches(market(None, Some(4.99)), &l, &m));
    assert!(
        !matches(market(None, None), &l, &HashMap::new()),
        "no market row → a market condition never matches"
    );
}

#[test]
fn text_conditions_take_comma_separated_terms() {
    let mut c = card("1", "Lightning Bolt", "1.00");
    c.set = "Magic 2010".to_string();
    c.set_code = "M10".to_string();
    c.location = Some("A1_S2_R3_C4".to_string());
    let l = Listing::from_card(0, &c, ListingFacts::default()).unwrap();
    let m = HashMap::new();
    assert!(matches(
        Condition::Name("counterspell, BOLT".into()),
        &l,
        &m
    ));
    assert!(!matches(Condition::Name("shock".into()), &l, &m));
    assert!(
        matches(Condition::Name(" , ".into()), &l, &m),
        "no terms = any"
    );
    assert!(matches(Condition::Set("m10".into()), &l, &m));
    assert!(matches(Condition::Set("magic 2010".into()), &l, &m));
    assert!(matches(Condition::Location("a1_s2".into()), &l, &m));
    assert!(!matches(Condition::Location("B1".into()), &l, &m));
}

#[test]
fn history_conditions_need_known_facts() {
    let mut l = listing("1", "1.00");
    let m = HashMap::new();
    let age = |min, max| Condition::AgeDays { min, max };
    assert!(
        !matches(age(Some(0), None), &l, &m),
        "unknown age never matches"
    );
    assert!(!matches(Condition::NoSaleDays(0), &l, &m));
    l.facts = ListingFacts {
        age_days: Some(100),
        last_sale: None,
        days_since_sale: Some(100),
        sold_30d: 2,
    };
    assert!(matches(age(Some(90), Some(100)), &l, &m));
    assert!(!matches(age(None, Some(99)), &l, &m));
    assert!(matches(Condition::NoSaleDays(100), &l, &m));
    assert!(!matches(Condition::NoSaleDays(101), &l, &m));
    assert!(matches(Condition::MinSold30d(2), &l, &m));
    assert!(!matches(Condition::MinSold30d(3), &l, &m));
}

#[test]
fn quantity_condition() {
    let mut c = card("1", "A", "1.00");
    c.quantity = "4".to_string();
    let l = Listing::from_card(0, &c, ListingFacts::default()).unwrap();
    assert!(matches(Condition::MinQuantity(4), &l, &HashMap::new()));
    assert!(!matches(Condition::MinQuantity(5), &l, &HashMap::new()));
}

#[test]
fn market_trend_condition_uses_rolling_averages() {
    let l = listing("1", "1.00");
    let mut r = row(1, Some(1.0));
    r.avg1 = Some(1.5);
    r.avg7 = Some(1.0);
    r.avg30 = Some(1.0);
    let m = market_of(vec![r]);
    assert!(matches(Condition::Moving(MarketMove::Rising), &l, &m));
    assert!(!matches(Condition::Moving(MarketMove::Falling), &l, &m));
    assert!(
        !matches(
            Condition::Moving(MarketMove::Flat),
            &l,
            &market_of(vec![row(1, Some(1.0))])
        ),
        "no averages → unknown direction → no match"
    );
}

// ── Rule order ──────────────────────────────────────────────────────────────

#[test]
fn first_enabled_matching_rule_wins() {
    let mut rs = plain(trend_action(), vec![]);
    rs.rules.insert(
        0,
        Rule {
            name: "disabled keep".into(),
            enabled: false,
            conditions: vec![],
            action: PriceAction {
                base: PriceBase::Keep,
                ..PriceAction::default()
            },
        },
    );
    rs.rules.insert(
        1,
        Rule {
            name: "commons only".into(),
            enabled: true,
            conditions: vec![Condition::Rarity(vec!["Common".into()])],
            action: PriceAction {
                base: PriceBase::Keep,
                ..PriceAction::default()
            },
        },
    );
    let l = listing("1", "1.00");
    let p = one(&rs, &l, &market_of(vec![row(1, Some(2.0))]));
    assert_eq!(p.rule, Some(2));
    assert_eq!(p.outcome, Outcome::Raise);
    assert_eq!(p.new_price, 2.0);
}

#[test]
fn no_rule_keeps_price() {
    let rs = plain(
        trend_action(),
        vec![Condition::Rarity(vec!["Common".into()])],
    );
    let p = one(&rs, &listing("1", "1.00"), &HashMap::new());
    assert_eq!(p.outcome, Outcome::NoRule);
    assert_eq!(p.new_price, 1.0);
    assert!(!p.default_ticked());
}

#[test]
fn keep_rule_protects_listing() {
    let rs = plain(
        PriceAction {
            base: PriceBase::Keep,
            ..PriceAction::default()
        },
        vec![],
    );
    let p = one(&rs, &listing("1", "0.01"), &HashMap::new());
    assert_eq!(p.outcome, Outcome::Kept);
    assert_eq!(p.new_price, 0.01, "keep even ignores floors");
}

// ── Bases ───────────────────────────────────────────────────────────────────

#[test]
fn market_base_combines_columns() {
    let l = listing("1", "1.00");
    let mut r = row(1, Some(2.0));
    r.avg30 = Some(3.0);
    let m = market_of(vec![r]);
    let base = |combine| PriceAction {
        base: PriceBase::Market {
            field: PriceField::Trend,
            combine,
            other: PriceField::Avg30,
        },
        adjust: false,
        ..PriceAction::default()
    };
    assert_eq!(
        one(&plain(base(Combine::Only), vec![]), &l, &m).new_price,
        2.0
    );
    assert_eq!(
        one(&plain(base(Combine::HigherOf), vec![]), &l, &m).new_price,
        3.0
    );
    assert_eq!(
        one(&plain(base(Combine::LowerOf), vec![]), &l, &m).new_price,
        2.0
    );

    // One column missing → the other one is used.
    let mut only_avg = row(1, None);
    only_avg.avg30 = Some(3.0);
    let m2 = market_of(vec![only_avg]);
    assert_eq!(
        one(&plain(base(Combine::LowerOf), vec![]), &l, &m2).new_price,
        3.0
    );
    assert_eq!(
        one(&plain(base(Combine::Only), vec![]), &l, &m2).outcome,
        Outcome::NoMarketData
    );
}

#[test]
fn foils_use_foil_columns_and_never_fall_back() {
    let mut c = card("1", "A", "1.00");
    c.is_foil = "true".to_string();
    let l = Listing::from_card(0, &c, ListingFacts::default()).unwrap();
    let rs = plain(trend_action(), vec![]);
    let p = one(&rs, &l, &market_of(vec![row(1, Some(2.0))]));
    assert_eq!(p.outcome, Outcome::NoMarketData);
    let mut r = row(1, Some(2.0));
    r.trend_foil = Some(5.0);
    assert_eq!(one(&rs, &l, &market_of(vec![r])).new_price, 5.0);
}

#[test]
fn stale_market_rows_are_skipped() {
    let mut r = row(1, Some(2.0));
    r.price_date = "2026-10-01".to_string();
    let m = market_of(vec![r]);
    let mut rs = plain(trend_action(), vec![]);
    assert_eq!(
        one(&rs, &listing("1", "1.00"), &m).outcome,
        Outcome::StaleMarket
    );
    rs.guardrails.stale_days = 0;
    assert_eq!(one(&rs, &listing("1", "1.00"), &m).outcome, Outcome::Raise);
}

#[test]
fn current_and_fixed_bases_need_no_market() {
    let current = plain(
        PriceAction {
            base: PriceBase::Current,
            percent: 90.0,
            ..PriceAction::default()
        },
        vec![],
    );
    assert_eq!(
        one(&current, &listing("1", "2.00"), &HashMap::new()).new_price,
        1.8
    );
    let fixed = plain(
        PriceAction {
            base: PriceBase::Fixed(0.25),
            ..PriceAction::default()
        },
        vec![],
    );
    assert_eq!(
        one(&fixed, &listing("1", "2.00"), &HashMap::new()).new_price,
        0.25
    );
}

#[test]
fn percent_then_offset() {
    let rs = plain(
        PriceAction {
            percent: 90.0,
            offset: -0.05,
            ..trend_action()
        },
        vec![],
    );
    let p = one(
        &rs,
        &listing("1", "1.00"),
        &market_of(vec![row(1, Some(2.0))]),
    );
    assert_eq!(p.new_price, 1.75);
}

#[test]
fn condition_and_language_factors_apply_to_market_base_only() {
    let mut c = card("1", "A", "1.00");
    c.condition = "EX".to_string();
    c.language = "French".to_string();
    let l = Listing::from_card(0, &c, ListingFacts::default()).unwrap();
    let mut rs = plain(PriceAction::default(), vec![]);
    rs.adjustments.condition_pct.insert("EX".into(), 90.0);
    rs.adjustments.language_pct.insert("french".into(), 50.0);
    let p = one(&rs, &l, &market_of(vec![row(1, Some(4.0))]));
    assert_eq!(p.new_price, 1.8, "4.00 × 90% × 50%");

    rs.rules[0].action.adjust = false;
    assert_eq!(
        one(&rs, &l, &market_of(vec![row(1, Some(4.0))])).new_price,
        4.0
    );

    rs.rules[0].action = PriceAction {
        base: PriceBase::Current,
        percent: 200.0,
        adjust: true,
        ..PriceAction::default()
    };
    assert_eq!(
        one(&rs, &l, &HashMap::new()).new_price,
        2.0,
        "current base not adjusted"
    );
}

// ── Limits, direction, floors ───────────────────────────────────────────────

#[test]
fn rule_min_max_and_step_limits() {
    let m = market_of(vec![row(1, Some(10.0))]);
    let rs = plain(
        PriceAction {
            max_price: Some(5.0),
            ..trend_action()
        },
        vec![],
    );
    assert_eq!(one(&rs, &listing("1", "1.00"), &m).new_price, 5.0);
    let rs = plain(
        PriceAction {
            max_raise_pct: Some(50.0),
            ..trend_action()
        },
        vec![],
    );
    assert_eq!(one(&rs, &listing("1", "2.00"), &m).new_price, 3.0);
    let rs = plain(
        PriceAction {
            max_drop_pct: Some(20.0),
            ..trend_action()
        },
        vec![],
    );
    assert_eq!(one(&rs, &listing("1", "20.00"), &m).new_price, 16.0);
    let rs = plain(
        PriceAction {
            min_price: Some(12.0),
            ..trend_action()
        },
        vec![],
    );
    assert_eq!(one(&rs, &listing("1", "20.00"), &m).new_price, 12.0);
}

#[test]
fn direction_holds_price() {
    let m = market_of(vec![row(1, Some(1.0))]);
    let raise_only = plain(
        PriceAction {
            direction: Direction::RaiseOnly,
            ..trend_action()
        },
        vec![],
    );
    let p = one(&raise_only, &listing("1", "2.00"), &m);
    assert_eq!(p.outcome, Outcome::Held);
    assert_eq!(p.new_price, 2.0);
    assert_eq!(
        one(&raise_only, &listing("1", "0.50"), &m).outcome,
        Outcome::Raise
    );

    let lower_only = plain(
        PriceAction {
            direction: Direction::LowerOnly,
            ..trend_action()
        },
        vec![],
    );
    assert_eq!(
        one(&lower_only, &listing("1", "0.50"), &m).outcome,
        Outcome::Held
    );
    assert_eq!(
        one(&lower_only, &listing("1", "2.00"), &m).outcome,
        Outcome::Lower
    );
}

#[test]
fn floors_win_over_rule_and_direction() {
    let mut rs = plain(
        PriceAction {
            base: PriceBase::Current,
            percent: 50.0,
            direction: Direction::LowerOnly,
            ..PriceAction::default()
        },
        vec![],
    );
    rs.guardrails.floors.rare = 0.30;
    // 1.00 × 50% = 0.50, above the floor.
    assert_eq!(
        one(&rs, &listing("1", "1.00"), &HashMap::new()).new_price,
        0.5
    );
    // 0.40 × 50% = 0.20 → lifted to the floor.
    assert_eq!(
        one(&rs, &listing("1", "0.40"), &HashMap::new()).new_price,
        0.3
    );
    // Already below the floor: "only lower" cannot keep it there.
    let p = one(&rs, &listing("1", "0.10"), &HashMap::new());
    assert_eq!(p.outcome, Outcome::Raise);
    assert_eq!(p.new_price, 0.3);
}

#[test]
fn floors_by_rarity_and_cardmarket_minimum() {
    let f = RarityFloors {
        common: 0.05,
        uncommon: 0.07,
        rare: 0.2,
        mythic: 0.4,
        other: 0.0,
    };
    assert_eq!(f.floor_for("Common"), 0.05);
    assert_eq!(f.floor_for("uncommon"), 0.07);
    assert_eq!(f.floor_for("Rare"), 0.2);
    assert_eq!(f.floor_for("Mythic"), 0.4);
    assert_eq!(f.floor_for("Token"), CARDMARKET_MIN_PRICE);
    assert_eq!(f.floor_for(""), CARDMARKET_MIN_PRICE);
}

#[test]
fn dead_band_ignores_small_changes_but_never_floor_lifts() {
    let mut rs = plain(trend_action(), vec![]);
    rs.guardrails.min_change_eur = 0.10;
    rs.guardrails.min_change_pct = 5.0;
    let m = market_of(vec![row(1, Some(10.30))]);
    // +€0.30 but only +3% → ignored.
    let p = one(&rs, &listing("1", "10.00"), &m);
    assert_eq!(p.outcome, Outcome::SmallChange);
    assert_eq!(p.new_price, 10.3, "the would-be price is still reported");
    assert!(!p.outcome.is_change());
    // +€0.05 (+50%) → ignored by the euro threshold.
    let m = market_of(vec![row(1, Some(0.15))]);
    assert_eq!(
        one(&rs, &listing("1", "0.10"), &m).outcome,
        Outcome::SmallChange
    );
    // A floor lift always applies.
    rs.guardrails.floors.rare = 0.12;
    let m = market_of(vec![row(1, Some(0.05))]);
    let p = one(&rs, &listing("1", "0.10"), &m);
    assert_eq!(p.outcome, Outcome::Raise);
    assert_eq!(p.new_price, 0.12);
}

#[test]
fn same_price_is_not_a_change() {
    let rs = plain(trend_action(), vec![]);
    let p = one(
        &rs,
        &listing("1", "2.00"),
        &market_of(vec![row(1, Some(2.004))]),
    );
    assert_eq!(p.outcome, Outcome::Same);
}

#[test]
fn big_changes_need_review() {
    let rs = plain(trend_action(), vec![]);
    // +100% and +€2 → review.
    let p = one(
        &rs,
        &listing("1", "2.00"),
        &market_of(vec![row(1, Some(4.0))]),
    );
    assert!(p.review);
    assert!(!p.default_ticked());
    // +100% but only +€0.10 → no review (cheap card).
    let p = one(
        &rs,
        &listing("1", "0.10"),
        &market_of(vec![row(1, Some(0.2))]),
    );
    assert!(!p.review);
    assert!(p.default_ticked());
    // +€2 but only +10% → no review.
    let p = one(
        &rs,
        &listing("1", "20.00"),
        &market_of(vec![row(1, Some(22.0))]),
    );
    assert!(!p.review);
}

// ── Rounding ────────────────────────────────────────────────────────────────

#[test]
fn rounding_grid() {
    use RoundMode::*;
    assert_eq!(round_cents(123, Rounding::Cent, Nearest), 123);
    assert_eq!(round_cents(0, Rounding::Cent, Nearest), 1);
    assert_eq!(round_cents(12, Rounding::FiveCents, Nearest), 10);
    assert_eq!(round_cents(13, Rounding::FiveCents, Nearest), 15);
    assert_eq!(
        round_cents(2, Rounding::FiveCents, Nearest),
        5,
        "never rounds to 0"
    );
    assert_eq!(round_cents(11, Rounding::FiveCents, Up), 15);
    assert_eq!(round_cents(14, Rounding::FiveCents, Down), 10);
    assert_eq!(round_cents(149, Rounding::TenCents, Nearest), 150);
    assert_eq!(round_cents(23, Rounding::EndsIn9, Nearest), 19);
    assert_eq!(round_cents(25, Rounding::EndsIn9, Nearest), 29);
    assert_eq!(round_cents(3, Rounding::EndsIn9, Nearest), 9);
    assert_eq!(round_cents(20, Rounding::EndsIn9, Up), 29);
    assert_eq!(round_cents(28, Rounding::EndsIn9, Down), 19);
    assert_eq!(round_cents(249, Rounding::EndsIn9, Up), 249);
}

#[test]
fn rounding_never_undercuts_the_floor() {
    let mut rs = plain(trend_action(), vec![]);
    rs.guardrails.rounding = Rounding::EndsIn9;
    rs.guardrails.floors.rare = 0.25;
    let p = one(
        &rs,
        &listing("1", "1.00"),
        &market_of(vec![row(1, Some(0.21))]),
    );
    assert_eq!(p.new_price, 0.29, "0.25 floor rounds up, not down to 0.19");
    let p = one(
        &rs,
        &listing("1", "1.00"),
        &market_of(vec![row(1, Some(2.33))]),
    );
    assert_eq!(p.new_price, 2.29);
}

#[test]
fn rounding_respects_rule_maximum() {
    let mut rs = plain(
        PriceAction {
            max_price: Some(2.0),
            ..trend_action()
        },
        vec![],
    );
    rs.guardrails.rounding = Rounding::EndsIn9;
    let p = one(
        &rs,
        &listing("1", "1.00"),
        &market_of(vec![row(1, Some(5.0))]),
    );
    assert_eq!(p.new_price, 1.99);
}

// ── Manual prices, stats, summary, export ───────────────────────────────────

#[test]
fn manual_price_wins_and_is_ticked() {
    let rs = plain(trend_action(), vec![]);
    let l = listing("1", "1.00");
    let manual: HashMap<usize, f64> = [(0, 7.5)].into_iter().collect();
    let eval = evaluate(
        &rs,
        std::slice::from_ref(&l),
        &market_of(vec![row(1, Some(2.0))]),
        &manual,
        today(),
    );
    let p = &eval.proposals[0];
    assert!(p.manual);
    assert_eq!(p.new_price, 7.5);
    assert_eq!(p.outcome, Outcome::Raise);
    assert!(p.default_ticked(), "a hand-typed price never needs review");
    assert_eq!(
        eval.rule_stats[0].matched, 0,
        "manual rows don't count for rules"
    );
}

#[test]
fn rule_stats_count_matches_and_value() {
    let rs = plain(trend_action(), vec![]);
    let mut a = card("1", "A", "1.00");
    a.quantity = "3".to_string();
    let b = card("2", "B", "5.00");
    let c = card("3", "C", "2.00");
    let listings = listings_from_cards(&[a, b, c], |_| ListingFacts::default());
    let market = market_of(vec![
        row(1, Some(2.0)),
        row(2, Some(4.0)),
        row(3, Some(2.0)),
    ]);
    let eval = evaluate(&rs, &listings, &market, &HashMap::new(), today());
    let s = eval.rule_stats[0];
    assert_eq!(s.matched, 3);
    assert_eq!(s.raised, 1);
    assert_eq!(s.lowered, 1);
    assert!((s.value_delta - (3.0 - 1.0)).abs() < 1e-9, "+€3 −€1");
}

#[test]
fn summary_and_export_follow_ticks() {
    let rs = plain(trend_action(), vec![]);
    let mut a = card("1", "A", "1.00");
    a.quantity = "2".to_string();
    let b = card("2", "B", "5.00");
    let c = card("3", "C", "2.00");
    let listings = listings_from_cards(&[a, b, c], |_| ListingFacts::default());
    // A → 1.50 (raise), B → 20.00 (+300%, review), C unchanged (no market).
    let market = market_of(vec![row(1, Some(1.5)), row(2, Some(20.0))]);
    let eval = evaluate(&rs, &listings, &market, &HashMap::new(), today());

    let s = summarize(&eval, &listings, |_, p| p.default_ticked());
    assert_eq!(s.raises, 2);
    assert_eq!(s.changes(), 2);
    assert_eq!(s.review, 1);
    assert_eq!(s.selected, 1);
    assert_eq!(s.selected_copies, 2);
    assert!((s.value_before - 9.0).abs() < 1e-9);
    assert!((s.value_after - 10.0).abs() < 1e-9);
    assert_eq!(s.skipped.get(Outcome::NoMarketData.label()), Some(&1));

    let changes = ticked_changes(&eval, &listings, |_, p| p.default_ticked());
    assert_eq!(
        changes,
        vec![PriceChange {
            row: 0,
            old_price: 1.0,
            new_price: 1.5
        }]
    );
    let all = ticked_changes(&eval, &listings, |_, _| true);
    assert_eq!(all.len(), 2, "unchanged rows are never exported");
}

#[test]
fn explain_lists_the_steps() {
    let mut rs = plain(
        PriceAction {
            percent: 90.0,
            ..trend_action()
        },
        vec![],
    );
    rs.guardrails.floors.rare = 3.0;
    let steps = explain(
        &rs,
        &listing("1", "1.00"),
        &market_of(vec![row(1, Some(2.0))]),
        None,
        today(),
    );
    let text = steps.join("\n");
    assert!(text.contains("Rule 1: r"), "{text}");
    assert!(text.contains("Market Trend €2.00"), "{text}");
    assert!(text.contains("× 90% = €1.80"), "{text}");
    assert!(text.contains("Floor for Rare €3.00"), "{text}");
    assert!(text.contains("Raise €1.00 to €3.00"), "{text}");

    let manual = explain(
        &rs,
        &listing("1", "1.00"),
        &HashMap::new(),
        Some(4.0),
        today(),
    );
    assert_eq!(manual, vec!["Set by hand to €4.00".to_string()]);
}

// ── Ruleset persistence ─────────────────────────────────────────────────────

#[test]
fn json_round_trip() {
    let rs = Ruleset::starter();
    let back = Ruleset::from_json(&rs.to_json()).unwrap();
    assert_eq!(back, rs);
}

#[test]
fn old_node_graph_files_are_refused() {
    let graph =
        r#"{"nodes":[],"wires":[],"canvas_offset_x":0,"canvas_offset_y":0,"canvas_zoom":1}"#;
    let err = Ruleset::from_json(graph).unwrap_err();
    assert!(err.contains("not a repricing ruleset"), "{err}");
    assert!(Ruleset::from_json("not json").is_err());
}

#[test]
fn missing_sections_take_defaults() {
    let rs = Ruleset::from_json(r#"{"rules":[{"name":"x"}]}"#).unwrap();
    assert_eq!(rs.rules[0].action, PriceAction::default());
    assert!(rs.rules[0].enabled);
    assert_eq!(rs.guardrails, Guardrails::default());
}

#[test]
fn sanitize_clamps_hostile_values() {
    let mut rs = Ruleset::starter();
    rs.name = "x".repeat(500);
    rs.rules[0].action.percent = f64::NAN;
    rs.rules[0].action.offset = -1e12;
    rs.rules[0].action.max_drop_pct = Some(500.0);
    rs.rules[0].action.base = PriceBase::Fixed(0.0);
    rs.rules[0].conditions = vec![Condition::ListedPrice {
        min: Some(f64::INFINITY),
        max: Some(-3.0),
    }];
    rs.guardrails.floors.common = f64::NEG_INFINITY;
    rs.guardrails.stale_days = u32::MAX;
    rs.adjustments.language_pct.insert("German".into(), 1e9);
    rs.rules
        .extend((0..200).map(|i| Rule::new(&format!("r{i}"))));
    rs.sanitize();

    assert_eq!(rs.name.chars().count(), MAX_NAME_CHARS);
    assert_eq!(rs.rules.len(), MAX_RULES);
    let a = &rs.rules[0].action;
    assert_eq!(a.percent, 100.0);
    assert_eq!(a.offset, -MAX_PRICE_EUR);
    assert_eq!(a.max_drop_pct, Some(100.0));
    assert_eq!(a.base, PriceBase::Fixed(CARDMARKET_MIN_PRICE));
    assert_eq!(
        rs.rules[0].conditions[0],
        Condition::ListedPrice {
            min: None,
            max: Some(0.0)
        }
    );
    assert_eq!(rs.guardrails.floors.common, CARDMARKET_MIN_PRICE);
    assert_eq!(rs.guardrails.stale_days, 3650);
    assert_eq!(rs.adjustments.language_pct["German"], MAX_PCT);
}

#[test]
fn save_and_load_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("rules.json");
    let rs = Ruleset::starter();
    save_ruleset(&path, &rs).unwrap();
    assert!(
        !path.with_extension("json.tmp").exists(),
        "temp file renamed away"
    );
    assert_eq!(load_ruleset(&path).unwrap(), rs);
}

#[test]
fn oversized_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.json");
    std::fs::write(&path, vec![b' '; (MAX_RULESET_FILE_BYTES + 1) as usize]).unwrap();
    assert!(load_ruleset(&path).unwrap_err().contains("too large"));
}

// ── Templates & descriptions ────────────────────────────────────────────────

#[test]
fn templates_have_unique_names_and_starter_uses_them() {
    let templates = rule_templates();
    let mut names: Vec<&str> = templates.iter().map(|t| t.rule.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), templates.len());
    let starter = Ruleset::starter();
    assert_eq!(starter.rules.len(), 4);
    assert!(starter
        .rules
        .iter()
        .all(|r| templates.iter().any(|t| t.rule == *r)));
    assert!(
        starter.rules.last().unwrap().conditions.is_empty(),
        "starter ends with a catch-all"
    );
}

#[test]
fn starter_ruleset_prices_a_realistic_listing() {
    let rs = Ruleset::starter();
    let mut c = card("1", "Bolt", "0.03");
    c.rarity = "Common".to_string();
    let l = Listing::from_card(0, &c, ListingFacts::default()).unwrap();
    let p = one(&rs, &l, &market_of(vec![row(1, Some(0.02))]));
    assert_eq!(p.rule, Some(3), "catch-all");
    assert_eq!(p.new_price, 0.05, "common floor");
}

#[test]
fn condition_templates_cover_every_kind_once() {
    let t = Condition::templates();
    for (i, a) in t.iter().enumerate() {
        for b in &t[i + 1..] {
            assert!(!a.same_kind(b), "{} twice", a.kind_label());
        }
    }
    assert_eq!(t.len(), 14);
}

#[test]
fn descriptions_read_naturally() {
    let r = Rule {
        name: "x".into(),
        enabled: true,
        conditions: vec![
            Condition::Rarity(vec!["Rare".into(), "Mythic".into()]),
            Condition::ListedPrice {
                min: Some(2.0),
                max: None,
            },
            Condition::AgeDays {
                min: Some(30),
                max: Some(90),
            },
        ],
        action: PriceAction {
            percent: 95.0,
            offset: -0.01,
            direction: Direction::LowerOnly,
            ..PriceAction::default()
        },
    };
    assert_eq!(
        r.describe_when(PriceField::Trend),
        "Rarity: Rare, Mythic · Listed ≥ €2.00 · In stock 30–90 days"
    );
    assert_eq!(
        r.action.describe(),
        "Trend × 95% − €0.01 · cond./lang. · only lower"
    );
    assert_eq!(
        Rule::new("y").describe_when(PriceField::Trend),
        "Every listing"
    );
    assert_eq!(
        PriceAction {
            base: PriceBase::Keep,
            ..PriceAction::default()
        }
        .describe(),
        "Keep current price"
    );
}
