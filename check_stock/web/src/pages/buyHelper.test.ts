import { afterEach, describe, expect, it, vi } from "vitest";
import type { BuyOfferParams } from "../api/types";
import { fileProblem, formFromParams, MAX_FILE_BYTES, paramsFromForm, parseDecimal, saveDownload } from "./buyHelper";

const defaults: BuyOfferParams = {
  single_common: false,
  single_uncommon: false,
  single_rare: true,
  single_mythic: true,
  min_single_price: 0.5,
  single_buy_percent: 60,
  bulk_rate: 10,
  bulk_batch: 1000,
};

describe("buy helper settings form", () => {
  it("round-trips the server's settings", () => {
    expect(paramsFromForm(formFromParams(defaults, defaults))).toEqual({ params: defaults });
    const noPriceRule = { ...defaults, min_single_price: null, single_common: true };
    const form = formFromParams(noPriceRule, defaults);
    expect(form.useMinPrice).toBe(false);
    expect(form.minPrice).toBe("0.5");
    expect(paramsFromForm(form)).toEqual({ params: noPriceRule });
  });

  it("reads decimal commas and points", () => {
    expect(parseDecimal("0,25")).toBe(0.25);
    expect(parseDecimal(" 12.5 ")).toBe(12.5);
    expect(parseDecimal("7")).toBe(7);
    for (const bad of ["", "-1", "1e3", "abc", "1.2.3", "Infinity", "NaN", "0x10", "1,000.5"]) {
      expect(parseDecimal(bad), bad).toBeNull();
    }
  });

  it("says what is wrong with each field", () => {
    const form = formFromParams(defaults, defaults);
    const check = paramsFromForm({ ...form, percent: "101", bulkRate: "-1", bulkBatch: "1.5", minPrice: "x" });
    expect(check.params).toBeUndefined();
    expect(Object.keys(check.errors ?? {}).sort()).toEqual(["bulkBatch", "bulkRate", "minPrice", "percent"]);
    // A disabled price rule is not checked.
    expect(paramsFromForm({ ...form, useMinPrice: false, minPrice: "x" }).params?.min_single_price).toBeNull();
    expect(paramsFromForm({ ...form, bulkBatch: "0" }).errors?.bulkBatch).toBeDefined();
    expect(paramsFromForm({ ...form, bulkBatch: "1000001" }).errors?.bulkBatch).toBeDefined();
    expect(paramsFromForm({ ...form, bulkRate: "1000001" }).errors?.bulkRate).toBeDefined();
    expect(paramsFromForm({ ...form, percent: "100" }).params?.single_buy_percent).toBe(100);
  });

  it("checks files before reading them", () => {
    expect(fileProblem({ name: "a.csv", size: 10 })).toBeNull();
    expect(fileProblem({ name: "a.csv", size: MAX_FILE_BYTES })).toBeNull();
    expect(fileProblem({ name: "a.csv", size: MAX_FILE_BYTES + 1 })).toMatch(/too large/);
    expect(fileProblem({ name: "a.csv", size: 0 })).toMatch(/empty/);
  });
});

describe("saving a download", () => {
  afterEach(() => vi.restoreAllMocks());

  it("hands the file to the browser under its name", () => {
    const createUrl = vi.fn(() => "blob:local/1");
    const revokeUrl = vi.fn();
    vi.stubGlobal("URL", { ...URL, createObjectURL: createUrl, revokeObjectURL: revokeUrl });
    const clicked: { download: string; href: string | null }[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      clicked.push({ download: this.download, href: this.getAttribute("href") });
    });
    vi.useFakeTimers();
    saveDownload({ blob: new Blob(["x"]), fileName: "buy_offer.csv" });
    expect(clicked).toEqual([{ download: "buy_offer.csv", href: "blob:local/1" }]);
    expect(document.querySelector("a[download]")).toBeNull();
    vi.advanceTimersByTime(10_000);
    expect(revokeUrl).toHaveBeenCalledWith("blob:local/1");
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });
});
