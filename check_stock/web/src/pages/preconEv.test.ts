import { describe, expect, it } from "vitest";
import {
  columnLabel,
  coverageText,
  formatRatio,
  isCardmarketUrl,
  jobText,
  lowCoverage,
  nextSort,
  ratioTone,
  SEALED_COLUMNS,
  soldText,
  soldTitle,
} from "./preconEv";

describe("precon EV helpers", () => {
  it("sorts like the desktop app", () => {
    expect(nextSort({ sort: "ratio", desc: true }, "ratio")).toEqual({ sort: "ratio", desc: false });
    expect(nextSort({ sort: "ratio", desc: false }, "ev")).toEqual({ sort: "ev", desc: true });
    expect(nextSort({ sort: "ratio", desc: true }, "name")).toEqual({ sort: "name", desc: false });
  });

  it("formats ratios and coverage", () => {
    expect(formatRatio(1.3125)).toBe("1.31×");
    expect(formatRatio(null)).toBe("—");
    expect(ratioTone({ ratio: 1.2, stale_sealed: null })).toBe("good");
    expect(ratioTone({ ratio: 0.8, stale_sealed: null })).toBe("plain");
    expect(ratioTone({ ratio: 1.5, stale_sealed: 60 })).toBe("stale");
    expect(ratioTone({ ratio: null, stale_sealed: 60 })).toBe("plain");
    expect(coverageText({ coverage: 0.975 })).toBe("98%");
    expect(coverageText(null)).toBe("—");
    expect(lowCoverage({ coverage: 0.94 })).toBe(true);
    expect(lowCoverage({ coverage: 0.95 })).toBe(false);
  });

  it("describes sales", () => {
    expect(soldText({ sale_days: 5, observed_days: 14 })).toBe("5/14 d");
    expect(soldText({ sale_days: 0, observed_days: 0 })).toBe("not checked");
    expect(soldText({ sale_days: null, observed_days: null })).toBe("—");
    expect(soldTitle({ sale_days: 7, observed_days: 14, liquidity: 1 })).toContain("counts 100% in Liquid EV");
    expect(soldTitle({ sale_days: 0, observed_days: 0, liquidity: 0 })).toContain("not checked");
    expect(soldTitle({ sale_days: null, observed_days: null, liquidity: null })).toBeUndefined();
  });

  it("labels columns and jobs", () => {
    expect(columnLabel("avg7")).toBe("Avg 7-day");
    expect(SEALED_COLUMNS.map((c) => c.value)).toEqual(["trend", "avg", "low"]);
    expect(jobText({ state: "running", started_at: "x", stage: "Loading decklists…", done: 3, total: 10 })).toBe(
      "Loading decklists… 3/10",
    );
    expect(jobText({ state: "running", started_at: "x", stage: "Fetching prices…", done: null, total: null })).toBe(
      "Fetching prices…",
    );
    expect(jobText({ state: "idle" })).toBe("");
  });

  it("links only to Cardmarket", () => {
    expect(isCardmarketUrl("https://www.cardmarket.com/en/Magic/Products?idProduct=1")).toBe(true);
    expect(isCardmarketUrl("javascript:alert(1)")).toBe(false);
    expect(isCardmarketUrl(null)).toBe(false);
  });
});
