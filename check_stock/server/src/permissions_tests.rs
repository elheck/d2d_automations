use super::*;
use std::collections::HashSet;

#[test]
fn keys_are_unique_and_round_trip() {
    let keys: HashSet<&str> = Permission::ALL.iter().map(|p| p.key()).collect();
    assert_eq!(keys.len(), Permission::ALL.len());
    for &p in Permission::ALL {
        assert_eq!(Permission::from_key(p.key()), Some(p));
    }
    assert_eq!(Permission::from_key("nope"), None);
    assert_eq!(Permission::from_key("USERS.MANAGE"), None, "keys are exact");
}

#[test]
fn keys_are_stable() {
    // Stored in the database: renaming one silently drops it for everyone.
    let keys: Vec<&str> = Permission::ALL.iter().map(|p| p.key()).collect();
    assert_eq!(
        keys,
        [
            "users.manage",
            "audit.view",
            "inventory.import",
            "inventory.discard",
            "search.use",
            "stock_checker.use",
            "bins.use",
            "analysis.view",
            "financials.view",
            "lot_costs.edit",
            "mispricing.view",
            "repricing.view",
            "repricing.edit_rules",
            "repricing.export",
            "bulk_lot.build",
            "bulk_lot.finish",
            "bulk_lot.manage",
            "precon_ev.use",
            "buy_helper.use",
            "listing.use",
        ]
    );
}

#[test]
fn every_permission_is_described_for_the_admin_interface() {
    let labels: HashSet<&str> = Permission::ALL.iter().map(|p| p.label()).collect();
    assert_eq!(labels.len(), Permission::ALL.len(), "labels are unique");
    for &p in Permission::ALL {
        assert!(!p.label().trim().is_empty(), "{p:?} has no label");
        assert!(
            p.description().len() >= 30,
            "{p:?} needs a real description, got {:?}",
            p.description()
        );
        assert!(p.description().ends_with('.'), "{p:?}: full sentences");
        assert!(!p.group().label().is_empty());
    }
}

#[test]
fn requirements_exist_and_never_form_a_cycle() {
    fn reaches(from: Permission, target: Permission, depth: usize) -> bool {
        depth < Permission::ALL.len()
            && from
                .requires()
                .iter()
                .any(|&r| r == target || reaches(r, target, depth + 1))
    }
    for &p in Permission::ALL {
        assert!(!p.requires().contains(&p), "{p:?} requires itself");
        assert!(!reaches(p, p, 0), "{p:?} is part of a cycle");
    }
}

#[test]
fn granting_needs_the_required_permissions() {
    let err = PermissionSet::from_keys(&["repricing.export"]).unwrap_err();
    assert!(
        err.contains("Export new prices") && err.contains("View repricing"),
        "{err}"
    );
    assert!(PermissionSet::from_keys(&["repricing.export", "repricing.view"]).is_ok());
    assert!(PermissionSet::from_keys(&["lot_costs.edit"]).is_err());
    assert!(PermissionSet::from_keys(&["inventory.discard"]).is_err());
    assert!(PermissionSet::from_keys(&["bins.use", "analysis.view"]).is_ok());
}

#[test]
fn unknown_keys_are_rejected() {
    let err = PermissionSet::from_keys(&["search.use", "root"]).unwrap_err();
    assert!(err.contains("root"), "{err}");
}

#[test]
fn the_full_set_is_consistent() {
    let all = PermissionSet::all();
    assert!(all.check_requirements().is_ok());
    assert_eq!(all.keys().len(), Permission::ALL.len());
    assert!(PermissionSet::new().is_empty());
    assert!(PermissionSet::new().check_requirements().is_ok());
}

#[test]
fn keys_come_in_display_order_without_duplicates() {
    let set = PermissionSet::from_keys(&["listing.use", "users.manage", "listing.use"]).unwrap();
    assert_eq!(set.keys(), ["users.manage", "listing.use"]);
    assert!(set.contains(Permission::ListingUse));
    assert!(!set.contains(Permission::AuditView));
}

#[test]
fn catalog_carries_texts_and_requirements() {
    let catalog = catalog();
    assert_eq!(catalog.len(), Permission::ALL.len());
    let export = catalog
        .iter()
        .find(|i| i.key == "repricing.export")
        .unwrap();
    assert_eq!(export.label, "Export new prices");
    assert_eq!(export.requires, ["repricing.view"]);
    assert_eq!(export.group, PermissionGroup::Pricing);
    assert_eq!(export.group_label, "Pricing");
    let json = serde_json::to_value(export).unwrap();
    assert_eq!(json["group"], "pricing");
    assert!(json["description"].as_str().unwrap().contains("CSV"));
}
