// The app's sections, grouped as in the navigation and on the start page.
// Each needs a permission; the server checks it again on every request.

import type { MeResponse } from "./api/types";
import { can } from "./api/queries";

export type NavPath =
  | "/singles"
  | "/buy-helper"
  | "/precon-ev"
  | "/restock"
  | "/movers"
  | "/mispricing"
  | "/stock-analysis"
  | "/inventory-import"
  | "/users"
  | "/audit";

export interface NavItem {
  to: NavPath;
  label: string;
  permission: string;
  /** One sentence for the start page. */
  description: string;
}

export interface NavGroup {
  label: string;
  items: NavItem[];
}

export const NAV_GROUPS: NavGroup[] = [
  {
    label: "Tools",
    items: [
      {
        to: "/singles",
        label: "Singles",
        permission: "listing.use",
        description: "Look up cards by set and collector number, with image and prices.",
      },
      {
        to: "/buy-helper",
        label: "Buy helper",
        permission: "buy_helper.use",
        description: "Value a seller's card list for a purchase offer.",
      },
      {
        to: "/inventory-import",
        label: "Inventory import",
        permission: "inventory.import",
        description: "Upload the Cardmarket inventory export; you see what changes before anything is written.",
      },
      {
        to: "/precon-ev",
        label: "Precon EV",
        permission: "precon_ev.use",
        description: "Rank Commander precons by the value of their singles against the sealed price.",
      },
    ],
  },
  {
    label: "Reports",
    items: [
      {
        to: "/restock",
        label: "Restock",
        permission: "analysis.view",
        description: "Sold-out cards ranked by how fast they sold: what to buy again.",
      },
      {
        to: "/movers",
        label: "Price movers",
        permission: "analysis.view",
        description: "Cards in stock whose market price rose or fell over 7 or 30 days.",
      },
      {
        to: "/mispricing",
        label: "Mispricing",
        permission: "mispricing.view",
        description: "Listings priced below or above the market, and what to do about each.",
      },
      {
        to: "/stock-analysis",
        label: "Stock analysis",
        permission: "analysis.view",
        description: "What is in stock, how fast it sells, how old it is and how each purchase lot is doing.",
      },
    ],
  },
  {
    label: "Admin",
    items: [
      {
        to: "/users",
        label: "Users",
        permission: "users.manage",
        description: "Create accounts and choose what each person may do.",
      },
      { to: "/audit", label: "Audit log", permission: "audit.view", description: "Who did what and when." },
    ],
  },
];

/** The groups with the sections `me` may open (empty groups left out). */
export function visibleGroups(me: MeResponse | null | undefined): NavGroup[] {
  return NAV_GROUPS.map((group) => ({ ...group, items: group.items.filter((i) => can(me, i.permission)) })).filter(
    (group) => group.items.length > 0,
  );
}

/** Whether `path` is (inside) one of the group's sections. */
export function groupIsActive(group: NavGroup, path: string): boolean {
  return group.items.some((i) => path === i.to || path.startsWith(`${i.to}/`));
}
