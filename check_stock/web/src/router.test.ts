import { describe, expect, it } from "vitest";
import { safeRedirect } from "./router";

describe("safeRedirect", () => {
  it("keeps paths inside the app", () => {
    expect(safeRedirect("/users")).toBe("/users");
    expect(safeRedirect("/audit?x=1")).toBe("/audit?x=1");
  });

  it("refuses anything that leaves the app or loops", () => {
    for (const bad of [
      "//evil.example",
      "/\\evil.example",
      "https://evil.example",
      "javascript:alert(1)",
      "users",
      "",
      "/login",
      "/login?redirect=/x",
      42,
      undefined,
      null,
    ]) {
      expect(safeRedirect(bad)).toBeUndefined();
    }
  });
});
