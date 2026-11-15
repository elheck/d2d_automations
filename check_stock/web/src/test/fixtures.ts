import type { AuditPage, JsonValue, MeResponse, PermissionInfo, SingleCard, UserView } from "../api/types";

export const catalog: PermissionInfo[] = [
  {
    key: "users.manage",
    label: "Manage users",
    description: "Create, change and delete user accounts. Grant it only to people you fully trust.",
    group: "administration",
    group_label: "Administration",
    requires: [],
  },
  {
    key: "audit.view",
    label: "View audit log",
    description: "See who did what and when.",
    group: "administration",
    group_label: "Administration",
    requires: [],
  },
  {
    key: "search.use",
    label: "Search cards",
    description: "Search the inventory and see prices.",
    group: "daily_work",
    group_label: "Daily work",
    requires: [],
  },
  {
    key: "inventory.discard",
    label: "Discard cards",
    description: "Write cards off as junk from Search.",
    group: "inventory",
    group_label: "Inventory",
    requires: ["search.use"],
  },
  {
    key: "repricing.view",
    label: "View repricing",
    description: "See the shared repricing rules.",
    group: "pricing",
    group_label: "Pricing",
    requires: [],
  },
  {
    key: "repricing.edit_rules",
    label: "Edit repricing rules",
    description: "Change the shared ruleset.",
    group: "pricing",
    group_label: "Pricing",
    requires: ["repricing.view"],
  },
  {
    key: "repricing.export",
    label: "Export new prices",
    description: "Export the ticked price changes as a CSV.",
    group: "pricing",
    group_label: "Pricing",
    requires: ["repricing.view"],
  },
  {
    key: "buy_helper.use",
    label: "Buy Helper",
    description:
      "Value a purchase offer from a seller's card list. The list itself is never saved; each user's split rules and rates are.",
    group: "tools",
    group_label: "Tools",
    requires: [],
  },
  {
    key: "listing.use",
    label: "Singles listing",
    description: "Look up cards by set and collector number, with images and prices.",
    group: "tools",
    group_label: "Tools",
    requires: [],
  },
];

export const admin: MeResponse = {
  username: "aron",
  display_name: "Aron",
  permissions: catalog.map((p) => p.key),
};

export const helper: MeResponse = { username: "helper", display_name: "Helper", permissions: ["search.use"] };

export const buyer: MeResponse = { username: "buyer", display_name: "Buyer", permissions: ["buy_helper.use"] };

export const lister: MeResponse = { username: "lister", display_name: "Lister", permissions: ["listing.use"] };

export function singleCard(overrides: Partial<SingleCard> = {}): SingleCard {
  return {
    set_code: "hou",
    collector_number: "120",
    name: "Lightning Bolt",
    set_name: "Hour of Devastation",
    rarity: "common",
    mana_cost: "{R}",
    type_line: "Instant",
    scryfall_eur: 0.25,
    scryfall_eur_foil: 1.1,
    cardmarket_id: 301234,
    cardmarket_url: "https://www.cardmarket.com/en/Magic/Products?idProduct=301234",
    image_path: "/api/singles/image/hou/120",
    prices: {
      status: "found",
      price_date: "2026-11-13",
      regular: { trend: 0.28, low: 0.02, avg: 0.3, avg1: 0.25, avg7: 0.27, avg30: 0.29 },
      foil: { trend: 1.05, low: null, avg: 1.2, avg1: null, avg7: 1.1, avg30: 1.15 },
    },
    ...overrides,
  };
}

export function userView(overrides: Partial<UserView> = {}): UserView {
  return {
    id: 1,
    username: "aron",
    display_name: "Aron",
    disabled: false,
    locked_until: null,
    permissions: admin.permissions,
    created_at: "2026-11-01T10:00:00Z",
    last_login_at: "2026-11-14T08:00:00Z",
    ...overrides,
  };
}

export function auditPage(ids: number[], next: number | null): AuditPage {
  return {
    entries: ids.map((id) => {
      const details: JsonValue =
        id % 2 ? { ip: "192.168.178.30" } : { permissions: { added: ["audit.view"], removed: [] } };
      return {
        id,
        at: "2026-11-14T08:00:00Z",
        actor: "aron",
        action: id % 2 ? "auth.login" : "user.updated",
        target: id % 2 ? null : "helper",
        details,
      };
    }),
    next_before_id: next,
  };
}
