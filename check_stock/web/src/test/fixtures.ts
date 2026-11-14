import type { AuditPage, JsonValue, MeResponse, PermissionInfo, UserView } from "../api/types";

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
];

export const admin: MeResponse = {
  username: "aron",
  display_name: "Aron",
  permissions: catalog.map((p) => p.key),
};

export const helper: MeResponse = { username: "helper", display_name: "Helper", permissions: ["search.use"] };

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
