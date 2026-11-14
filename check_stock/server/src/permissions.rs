//! What a user may do. Each user has an explicit set of permissions, granted
//! one by one in the user administration.
//!
//! Every permission carries a label, a description and a group for the admin
//! interface (served by `GET /api/permissions`), and the permissions it
//! needs: e.g. exporting new prices is only possible together with seeing
//! the repricing screen. The list below is the single source of truth: the
//! enum, [`Permission::ALL`] and the texts are generated from it, so no
//! permission can lack a description or be missing from the list.

use serde::Serialize;
use std::collections::BTreeSet;
use ts_rs::TS;

/// Groups shown as sections in the user administration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PermissionGroup {
    Administration,
    Inventory,
    DailyWork,
    Reports,
    Pricing,
    BulkLots,
    Tools,
}

impl PermissionGroup {
    pub fn label(self) -> &'static str {
        match self {
            PermissionGroup::Administration => "Administration",
            PermissionGroup::Inventory => "Inventory",
            PermissionGroup::DailyWork => "Daily work",
            PermissionGroup::Reports => "Reports and money",
            PermissionGroup::Pricing => "Pricing",
            PermissionGroup::BulkLots => "Bulk lots",
            PermissionGroup::Tools => "Tools",
        }
    }
}

macro_rules! permissions {
    ($(
        $variant:ident = $key:literal, $group:ident, $label:literal, [$($needs:ident),*],
        $description:literal;
    )*) => {
        /// One permission. See the module docs.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Permission {
            $($variant),*
        }

        impl Permission {
            /// Every permission, in the order the admin interface lists them.
            pub const ALL: &'static [Permission] = &[$(Permission::$variant),*];

            /// Stable key used in the database and the API, e.g. `users.manage`.
            pub fn key(self) -> &'static str {
                match self { $(Permission::$variant => $key),* }
            }

            /// Short name for the admin interface.
            pub fn label(self) -> &'static str {
                match self { $(Permission::$variant => $label),* }
            }

            /// What the permission allows, for the admin interface.
            pub fn description(self) -> &'static str {
                match self { $(Permission::$variant => $description),* }
            }

            pub fn group(self) -> PermissionGroup {
                match self { $(Permission::$variant => PermissionGroup::$group),* }
            }

            /// Permissions that must be granted together with this one.
            pub fn requires(self) -> &'static [Permission] {
                match self { $(Permission::$variant => &[$(Permission::$needs),*]),* }
            }
        }
    };
}

permissions! {
    UsersManage = "users.manage", Administration, "Manage users", [],
        "Create, change and delete user accounts, set passwords and grant permissions. Whoever has this can give themselves every other permission, so grant it only to people you fully trust.";
    AuditView = "audit.view", Administration, "View audit log", [],
        "See the log of who did what and when: logins, imports, discards, price exports and user changes.";
    InventoryImport = "inventory.import", Inventory, "Import inventory", [],
        "Upload Cardmarket inventory exports. An import changes the stock for everyone and records copies that are gone as sold, so it affects every report.";
    InventoryDiscard = "inventory.discard", Inventory, "Discard cards", [SearchUse],
        "Write cards off as junk from Search: lowers the stock without counting a sale and creates the stock-update CSV for Cardmarket.";
    SearchUse = "search.use", DailyWork, "Search cards", [],
        "Search the inventory, see prices and price history, and send cards to the Stock Checker.";
    StockCheckerUse = "stock_checker.use", DailyWork, "Stock Checker and picking", [],
        "Match wantslists and deck lists against the stock, pick orders and export picking lists and stock-update CSVs.";
    BinsUse = "bins.use", DailyWork, "Bin moves", [AnalysisView],
        "Work through bin consolidation move lists and mark moves as done. Never changes the stock.";
    AnalysisView = "analysis.view", Reports, "View reports", [],
        "See stock analysis, aging, bin analysis, restock and price movers. Money figures (revenue, margins, lot costs) also need \"View revenue and costs\".";
    FinancialsView = "financials.view", Reports, "View revenue and costs", [],
        "See revenue, margins, purchase-lot costs and money totals wherever they appear (welcome page, reports, bulk lots). Without it the server leaves these figures out.";
    LotCostsEdit = "lot_costs.edit", Reports, "Edit lot costs", [FinancialsView],
        "Enter and correct what a purchase lot cost.";
    MispricingView = "mispricing.view", Reports, "Mispricing report", [],
        "See listings priced below or above the market. Read-only: changing prices needs the repricing permissions.";
    RepricingView = "repricing.view", Pricing, "View repricing", [],
        "See the shared repricing rules and the price changes they propose.";
    RepricingEditRules = "repricing.edit_rules", Pricing, "Edit repricing rules", [RepricingView],
        "Change the shared repricing ruleset that everyone uses.";
    RepricingExport = "repricing.export", Pricing, "Export new prices", [RepricingView],
        "Export the ticked price changes as a CSV for Cardmarket, with its undo file. Importing that file is what actually changes the prices.";
    BulkLotBuild = "bulk_lot.build", BulkLots, "Build bulk lots", [],
        "Choose cards for a bulk lot and pick them from the shelves.";
    BulkLotFinish = "bulk_lot.finish", BulkLots, "Finish bulk lots", [BulkLotBuild],
        "Record a bulk lot with its price and export its stock-update CSV. This reserves the cards and books the lot's revenue.";
    BulkLotManage = "bulk_lot.manage", BulkLots, "Manage bulk lots", [BulkLotBuild],
        "Correct the price of recorded bulk lots, mark their removal as done, or undo a lot.";
    PreconEvUse = "precon_ev.use", Tools, "Precon EV", [],
        "Rank Commander precons by the value of their singles against the sealed price.";
    BuyHelperUse = "buy_helper.use", Tools, "Buy Helper", [],
        "Value a purchase offer from a card list. Nothing is saved.";
    ListingUse = "listing.use", Tools, "Singles listing", [],
        "Look up cards by set and collector number, with images and prices.";
}

impl Permission {
    /// The permission with this key, if any.
    pub fn from_key(key: &str) -> Option<Permission> {
        Permission::ALL.iter().copied().find(|p| p.key() == key)
    }
}

/// A user's permissions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionSet(BTreeSet<Permission>);

impl PermissionSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every permission (the first admin gets these).
    pub fn all() -> Self {
        Self(Permission::ALL.iter().copied().collect())
    }

    /// Parses API input. Unknown keys and missing requirements are errors.
    pub fn from_keys<S: AsRef<str>>(keys: &[S]) -> Result<Self, String> {
        let mut set = BTreeSet::new();
        for key in keys {
            let key = key.as_ref();
            let permission =
                Permission::from_key(key).ok_or_else(|| format!("unknown permission {key:?}"))?;
            set.insert(permission);
        }
        let set = Self(set);
        set.check_requirements()?;
        Ok(set)
    }

    /// Every granted permission needs the ones it requires granted too.
    pub fn check_requirements(&self) -> Result<(), String> {
        for permission in &self.0 {
            for needed in permission.requires() {
                if !self.0.contains(needed) {
                    return Err(format!(
                        "\"{}\" also needs \"{}\"",
                        permission.label(),
                        needed.label()
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn contains(&self, permission: Permission) -> bool {
        self.0.contains(&permission)
    }

    pub fn insert(&mut self, permission: Permission) {
        self.0.insert(permission);
    }

    pub fn iter(&self) -> impl Iterator<Item = Permission> + '_ {
        self.0.iter().copied()
    }

    /// Keys in [`Permission::ALL`] order.
    pub fn keys(&self) -> Vec<&'static str> {
        Permission::ALL
            .iter()
            .filter(|p| self.0.contains(p))
            .map(|p| p.key())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One permission as the admin interface shows it.
#[derive(Debug, Serialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct PermissionInfo {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub group: PermissionGroup,
    pub group_label: &'static str,
    pub requires: Vec<&'static str>,
}

/// Every permission with its texts, in display order.
pub fn catalog() -> Vec<PermissionInfo> {
    Permission::ALL
        .iter()
        .map(|&p| PermissionInfo {
            key: p.key(),
            label: p.label(),
            description: p.description(),
            group: p.group(),
            group_label: p.group().label(),
            requires: p.requires().iter().map(|r| r.key()).collect(),
        })
        .collect()
}

#[cfg(test)]
#[path = "permissions_tests.rs"]
mod tests;
