import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_SET_KEY,
  isCardmarketUrl,
  loadDefaultSet,
  parseCardInput,
  priceStatusText,
  resolveCardInput,
  saveDefaultSet,
} from "./singles";

describe("card input", () => {
  // Same cases as check_stock_core's card_input tests.
  it("reads a set code and three digits", () => {
    expect(parseCardInput("hou120")).toEqual({ setCode: "hou", collectorNumber: "120" });
    expect(parseCardInput("MH2130")).toEqual({ setCode: "mh2", collectorNumber: "130" });
    expect(parseCardInput("2xm050")).toEqual({ setCode: "2xm", collectorNumber: "50" });
    expect(parseCardInput("  lea005 ")).toEqual({ setCode: "lea", collectorNumber: "5" });
    expect(parseCardInput("lea000")).toEqual({ setCode: "lea", collectorNumber: "0" });
  });

  it("refuses everything else", () => {
    for (const bad of ["", "120", "hou", "hou12a", "hou-12", "abc", "äöü120", "hou 120x", "../120", "h/u120"]) {
      expect(parseCardInput(bad), bad).toBeNull();
    }
    expect(parseCardInput("a".repeat(11) + "120")).toBeNull();
  });

  it("uses the default set for three digits", () => {
    expect(resolveCardInput("120", "HOU")).toEqual({ setCode: "hou", collectorNumber: "120" });
    expect(resolveCardInput("007", " dmu ")).toEqual({ setCode: "dmu", collectorNumber: "7" });
    expect(resolveCardInput("120", "")).toBeNull();
    expect(resolveCardInput("mh2130", "hou")).toEqual({ setCode: "mh2", collectorNumber: "130" });
    expect(resolveCardInput("   ", "hou")).toBeNull();
    expect(resolveCardInput("120", "../")).toBeNull();
  });
});

describe("price texts", () => {
  it("explains missing prices", () => {
    expect(priceStatusText({ status: "not_tracked" })).toMatch(/no prices/);
    expect(priceStatusText({ status: "unavailable" })).toMatch(/did not answer/);
    expect(priceStatusText({ status: "not_configured" })).toMatch(/configured/);
    expect(priceStatusText({ status: "no_cardmarket_id" })).toMatch(/Cardmarket product/);
  });

  it("links only to Cardmarket", () => {
    expect(isCardmarketUrl("https://www.cardmarket.com/en/Magic/Products?idProduct=1")).toBe(true);
    expect(isCardmarketUrl(null)).toBe(false);
    expect(isCardmarketUrl("javascript:alert(1)")).toBe(false);
    expect(isCardmarketUrl("https://www.cardmarket.com.evil.example/")).toBe(false);
    expect(isCardmarketUrl("http://www.cardmarket.com/")).toBe(false);
  });
});

describe("default set", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    window.localStorage.clear();
  });

  it("is remembered on this device", () => {
    expect(loadDefaultSet()).toBe("");
    saveDefaultSet("hou");
    expect(window.localStorage.getItem(DEFAULT_SET_KEY)).toBe("hou");
    expect(loadDefaultSet()).toBe("hou");
    saveDefaultSet("");
    expect(window.localStorage.getItem(DEFAULT_SET_KEY)).toBeNull();
  });

  it("works without storage", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    expect(loadDefaultSet()).toBe("");
    expect(() => saveDefaultSet("hou")).not.toThrow();
  });
});
