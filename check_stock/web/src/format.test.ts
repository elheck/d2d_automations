import { describe, expect, it } from "vitest";
import { formatCount, formatEuro } from "./format";

describe("formats", () => {
  it("formats euros", () => {
    expect(formatEuro(0.25)).toBe("0.25 €");
    expect(formatEuro(12)).toBe("12.00 €");
    expect(formatEuro(9010.94)).toBe("9010.94 €");
    expect(formatEuro(null)).toBe("—");
  });

  it("formats counts", () => {
    expect(formatCount(3)).toBe("3");
    expect(formatCount(1004)).toBe("1,004");
  });
});
