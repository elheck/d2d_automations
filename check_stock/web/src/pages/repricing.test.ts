import { describe, expect, it } from "vitest";
import type { ConditionTemplate, Rule, Ruleset } from "../api/types";
import {
  addRule,
  baseKind,
  baseOf,
  blankRule,
  conditionKind,
  deleteRule,
  duplicateRule,
  freeName,
  freeTemplates,
  hoursSince,
  isBigExport,
  moveRule,
  nextSort,
  numberText,
  parseNumber,
  sameRuleset,
  signedEuro,
  signedPercent,
  templateOf,
} from "./repricing";

function rule(name: string, conditions: Rule["conditions"] = []): Rule {
  return { ...blankRule(), name, conditions };
}

function ruleset(...rules: Rule[]): Ruleset {
  return {
    version: 1,
    name: "Test",
    rules,
    guardrails: {
      floors: { common: 0.05, uncommon: 0.05, rare: 0.1, mythic: 0.2, other: 0.05 },
      rounding: "Cent",
      min_change_eur: 0.01,
      min_change_pct: 2,
      stale_days: 7,
      review_pct: 30,
      review_min_eur: 1,
      reference: "Trend",
    },
    adjustments: { condition_pct: {}, language_pct: {} },
  };
}

const templates: ConditionTemplate[] = [
  { label: "Rarity", help: "Rarity is one of the picked ones", condition: { Rarity: [] } },
  { label: "Name", help: "Card name contains a word", condition: { Name: "" } },
];

describe("repricing helpers", () => {
  it("sorts text A→Z and numbers largest first, flipping on the same column", () => {
    expect(nextSort({ sort: "impact", desc: true }, "name")).toEqual({ sort: "name", desc: false });
    expect(nextSort({ sort: "name", desc: false }, "change")).toEqual({ sort: "change", desc: true });
    expect(nextSort({ sort: "change", desc: true }, "change")).toEqual({ sort: "change", desc: false });
    expect(nextSort({ sort: "impact", desc: true }, "rule")).toEqual({ sort: "rule", desc: false });
  });

  it("knows how old an import is", () => {
    const now = new Date("2026-11-17T12:00:00Z");
    expect(hoursSince("2026-11-16T12:00:00Z", now)).toBe(24);
    expect(hoursSince("2026-11-18T12:00:00Z", now)).toBe(0);
    expect(hoursSince("not a time", now)).toBe(0);
  });

  it("calls an export big above a quarter of the listings", () => {
    expect(isBigExport(25, 100)).toBe(false);
    expect(isBigExport(26, 100)).toBe(true);
    expect(isBigExport(1, 0)).toBe(false);
  });

  it("writes signed money and percentages", () => {
    expect(signedEuro(1.2)).toBe("+1.20 €");
    expect(signedEuro(-0.354)).toBe("−0.35 €");
    expect(signedEuro(0.001)).toBe("0.00 €");
    expect(signedPercent(11.11)).toBe("+11%");
    expect(signedPercent(-4.5)).toBe("−4.5%");
    expect(signedPercent(-4.04)).toBe("−4%");
    expect(signedPercent(0)).toBe("0%");
  });

  it("reads typed numbers with a comma or a point, within limits", () => {
    const price = { min: 0, max: 100, optional: true };
    expect(parseNumber("6,50", price)).toBe(6.5);
    expect(parseNumber(" 7.25 ", price)).toBe(7.25);
    expect(parseNumber("", price)).toBeNull();
    expect(parseNumber("", { min: 0, max: 1 })).toBeUndefined();
    expect(parseNumber("101", price)).toBeUndefined();
    expect(parseNumber("-1", price)).toBeUndefined();
    expect(parseNumber("-1", { min: -5, max: 5 })).toBe(-1);
    expect(parseNumber("1e3", price)).toBeUndefined();
    expect(parseNumber("2.5", { min: 0, max: 10, integer: true })).toBeUndefined();
    expect(parseNumber("12", { min: 0, max: 100, integer: true })).toBe(12);
    expect(numberText(null)).toBe("");
    expect(numberText(90)).toBe("90");
    expect(numberText(0.1 + 0.2)).toBe("0.3");
  });

  it("finds condition kinds and the kinds still free", () => {
    expect(conditionKind({ Rarity: ["Rare"] })).toBe("Rarity");
    expect(conditionKind({ ListedPrice: { min: 1, max: null } })).toBe("ListedPrice");
    expect(templateOf({ Name: "bolt" }, templates)?.label).toBe("Name");
    expect(templateOf({ MinQuantity: 4 }, templates)).toBeUndefined();
    expect(freeTemplates(rule("r", [{ Rarity: ["Common"] }]), templates).map((t) => t.label)).toEqual(["Name"]);
  });

  it("switches price bases and keeps what fits", () => {
    const market: Rule["action"]["base"] = { Market: { field: "Low", combine: "HigherOf", other: "Trend" } };
    expect(baseKind(market)).toBe("Market");
    expect(baseKind({ Fixed: 2 })).toBe("Fixed");
    expect(baseKind("Keep")).toBe("Keep");
    expect(baseOf("Market", market)).toBe(market);
    expect(baseOf("Market", "Current")).toEqual({ Market: { field: "Trend", combine: "Only", other: "Avg30" } });
    expect(baseOf("Fixed", { Fixed: 3 })).toEqual({ Fixed: 3 });
    expect(baseOf("Fixed", market)).toEqual({ Fixed: 1 });
    expect(baseOf("Current", market)).toBe("Current");
  });

  it("edits the rule order without touching the original", () => {
    const original = ruleset(rule("A"), rule("B"), rule("C"));
    const moved = moveRule(original, 0, 1);
    expect(moved.rules.map((r) => r.name)).toEqual(["B", "A", "C"]);
    expect(original.rules.map((r) => r.name)).toEqual(["A", "B", "C"]);
    expect(moveRule(original, 0, -1)).toBe(original);
    expect(moveRule(original, 2, 1)).toBe(original);
    expect(deleteRule(original, 1).rules.map((r) => r.name)).toEqual(["A", "C"]);
    const copied = duplicateRule(original, 0);
    expect(copied.rules.map((r) => r.name)).toEqual(["A", "A copy", "B", "C"]);
    expect(duplicateRule(copied, 0).rules[1]?.name).toBe("A copy (2)");
    expect(freeName("X", ["X", "X (2)"])).toBe("X (3)");
  });

  it("adds a rule before a catch-all, so the new rule gets a chance", () => {
    const withCatchAll = ruleset(rule("Protect", [{ Rarity: ["Mythic"] }]), rule("Follow"));
    const added = addRule(withCatchAll, rule("Follow"));
    expect(added.index).toBe(1);
    expect(added.ruleset.rules.map((r) => r.name)).toEqual(["Protect", "Follow (2)", "Follow"]);
    const narrow = ruleset(rule("Protect", [{ Rarity: ["Mythic"] }]));
    expect(addRule(narrow, rule("New")).index).toBe(1);
    expect(addRule(ruleset(), rule("New")).index).toBe(0);
    expect(sameRuleset(added.ruleset, structuredClone(added.ruleset))).toBe(true);
    expect(sameRuleset(added.ruleset, withCatchAll)).toBe(false);
  });
});
