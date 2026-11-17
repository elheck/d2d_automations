// Rules and texts of the Repricing page (tested without rendering).

import type {
  ConditionTemplate,
  OutcomeKey,
  PriceBase,
  ReviewFilter,
  ReviewSort,
  Rule,
  RuleCondition,
  Ruleset,
} from "../api/types";

export const FILTERS: { key: ReviewFilter; label: string }[] = [
  { key: "changes", label: "Changes" },
  { key: "raises", label: "Raises" },
  { key: "lowers", label: "Drops" },
  { key: "needs_review", label: "Needs review" },
  { key: "unchanged", label: "Unchanged" },
  { key: "held_back", label: "Held back" },
  { key: "all", label: "All" },
];

/** Rows per page of the review. */
export const PAGE_SIZE = 100;
/** An import older than this is warned about at the export. */
export const IMPORT_STALE_HOURS = 24;
/** An export changing more than this share of all listings is warned about. */
export const BIG_EXPORT_SHARE = 0.25;

/** Same column: flip; another: text columns A→Z, numbers largest first. */
export function nextSort(current: { sort: ReviewSort; desc: boolean }, column: ReviewSort) {
  if (current.sort === column) return { sort: column, desc: !current.desc };
  return { sort: column, desc: !["name", "set", "rule"].includes(column) };
}

/** Hours since an RFC 3339 time (0 when it cannot be read or lies ahead). */
export function hoursSince(time: string, now: Date): number {
  const at = Date.parse(time);
  if (Number.isNaN(at)) return 0;
  return Math.max(0, (now.getTime() - at) / 3_600_000);
}

export function isBigExport(selected: number, listings: number): boolean {
  return listings > 0 && selected / listings > BIG_EXPORT_SHARE;
}

/** `+1.20 €` / `−0.35 €` / `0.00 €`. */
export function signedEuro(value: number): string {
  const cents = Math.round(value * 100);
  if (cents === 0) return "0.00 €";
  return `${cents > 0 ? "+" : "−"}${(Math.abs(cents) / 100).toFixed(2)} €`;
}

/** `+12%` / `−4.5%` (one decimal below 10%). */
export function signedPercent(value: number): string {
  if (value === 0) return "0%";
  const abs = Math.abs(value);
  const text = abs < 10 ? abs.toFixed(1).replace(/\.0$/, "") : Math.round(abs).toString();
  return `${value > 0 ? "+" : "−"}${text}%`;
}

export const OUTCOME_TONE: Record<OutcomeKey, "up" | "down" | "none"> = {
  raise: "up",
  lower: "down",
  no_rule: "none",
  kept: "none",
  no_market_data: "none",
  stale_market: "none",
  no_price: "none",
  held: "none",
  small_change: "none",
  same: "none",
};

// ── Numbers typed by hand ───────────────────────────────────────────────────

export interface NumberRules {
  min: number;
  max: number;
  /** Whole numbers only. */
  integer?: boolean;
  /** Empty text means "none" (`null`). */
  optional?: boolean;
}

/**
 * A typed number (comma or point): the value, `null` for an empty optional
 * field, or `undefined` when the text is not a valid value.
 */
export function parseNumber(text: string, rules: NumberRules): number | null | undefined {
  const t = text.trim().replace(",", ".");
  if (t === "") return rules.optional ? null : undefined;
  const pattern = rules.integer ? /^-?\d+$/ : /^-?\d+(\.\d+)?$/;
  if (!pattern.test(t)) return undefined;
  const value = Number(t);
  if (!Number.isFinite(value) || value < rules.min || value > rules.max) return undefined;
  return value;
}

/** How a number shows in its field (`null`: empty). */
export function numberText(value: number | null): string {
  if (value === null) return "";
  return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(4)));
}

// ── Conditions and price bases ──────────────────────────────────────────────

/** The kind of a condition (its variant name, e.g. `Rarity`). */
export function conditionKind(condition: RuleCondition): string {
  return Object.keys(condition)[0] ?? "";
}

/** The menu entry of a condition's kind. */
export function templateOf(condition: RuleCondition, templates: ConditionTemplate[]): ConditionTemplate | undefined {
  const kind = conditionKind(condition);
  return templates.find((t) => conditionKind(t.condition) === kind);
}

/** Kinds a rule can still get (one condition per kind, as on the desktop). */
export function freeTemplates(rule: Rule, templates: ConditionTemplate[]): ConditionTemplate[] {
  const taken = new Set(rule.conditions.map(conditionKind));
  return templates.filter((t) => !taken.has(conditionKind(t.condition)));
}

export type BaseKind = "Market" | "Current" | "Fixed" | "Keep";

export const BASE_KINDS: { key: BaseKind; label: string }[] = [
  { key: "Market", label: "Market price" },
  { key: "Current", label: "Current price" },
  { key: "Fixed", label: "Fixed price" },
  { key: "Keep", label: "Keep the price" },
];

export function baseKind(base: PriceBase): BaseKind {
  if (base === "Current" || base === "Keep") return base;
  return "Market" in base ? "Market" : "Fixed";
}

/** A base of `kind`, keeping what the previous one had where it fits. */
export function baseOf(kind: BaseKind, previous: PriceBase): PriceBase {
  if (kind === "Current" || kind === "Keep") return kind;
  if (kind === "Market") {
    return typeof previous === "object" && "Market" in previous
      ? previous
      : { Market: { field: "Trend", combine: "Only", other: "Avg30" } };
  }
  return typeof previous === "object" && "Fixed" in previous ? previous : { Fixed: 1 };
}

// ── Editing the ruleset (never in place) ────────────────────────────────────

export function updateRule(ruleset: Ruleset, index: number, rule: Rule): Ruleset {
  return { ...ruleset, rules: ruleset.rules.map((r, i) => (i === index ? rule : r)) };
}

/** Moves rule `index` one up (`-1`) or down (`+1`); the order decides which rule wins. */
export function moveRule(ruleset: Ruleset, index: number, step: -1 | 1): Ruleset {
  const target = index + step;
  const moving = ruleset.rules[index];
  const other = ruleset.rules[target];
  if (!moving || !other) return ruleset;
  const rules = [...ruleset.rules];
  rules[index] = other;
  rules[target] = moving;
  return { ...ruleset, rules };
}

export function deleteRule(ruleset: Ruleset, index: number): Ruleset {
  return { ...ruleset, rules: ruleset.rules.filter((_, i) => i !== index) };
}

/** `name`, or `name (2)`, `name (3)`, … when taken. */
export function freeName(name: string, taken: string[]): string {
  if (!taken.includes(name)) return name;
  let n = 2;
  while (taken.includes(`${name} (${n})`)) n++;
  return `${name} (${n})`;
}

/** A copy right below the original. */
export function duplicateRule(ruleset: Ruleset, index: number): Ruleset {
  const original = ruleset.rules[index];
  if (!original) return ruleset;
  const copy: Rule = {
    ...structuredClone(original),
    name: freeName(
      `${original.name} copy`,
      ruleset.rules.map((r) => r.name),
    ),
  };
  const rules = [...ruleset.rules];
  rules.splice(index + 1, 0, copy);
  return { ...ruleset, rules };
}

/**
 * Adds a rule above the last one when that is a catch-all (no conditions),
 * so it gets a chance to match; else at the end. Returns the new index.
 */
export function addRule(ruleset: Ruleset, rule: Rule): { ruleset: Ruleset; index: number } {
  const copy: Rule = {
    ...structuredClone(rule),
    name: freeName(
      rule.name,
      ruleset.rules.map((r) => r.name),
    ),
  };
  const last = ruleset.rules.at(-1);
  const index = last && last.enabled && last.conditions.length === 0 ? ruleset.rules.length - 1 : ruleset.rules.length;
  const rules = [...ruleset.rules];
  rules.splice(index, 0, copy);
  return { ruleset: { ...ruleset, rules }, index };
}

/** A rule following the market trend (the "blank" rule of the menu). */
export function blankRule(): Rule {
  return {
    name: "New rule",
    enabled: true,
    conditions: [],
    action: {
      base: { Market: { field: "Trend", combine: "Only", other: "Avg30" } },
      percent: 100,
      offset: 0,
      adjust: true,
      direction: "Both",
      min_price: null,
      max_price: null,
      max_raise_pct: null,
      max_drop_pct: null,
    },
  };
}

/** Same content (rulesets compare by value). */
export function sameRuleset(a: Ruleset, b: Ruleset): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
