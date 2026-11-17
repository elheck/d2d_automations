import { describe, expect, it } from "vitest";
import { groupIsActive, NAV_GROUPS, visibleGroups } from "./navigation";

const me = (permissions: string[]) => ({ username: "u", display_name: "U", permissions });

describe("navigation", () => {
  it("shows only groups with sections the user may open", () => {
    expect(visibleGroups(me([])).map((g) => g.label)).toEqual([]);
    const groups = visibleGroups(me(["listing.use", "audit.view"]));
    expect(groups.map((g) => g.label)).toEqual(["Tools", "Admin"]);
    expect(groups[0]?.items.map((i) => i.to)).toEqual(["/singles"]);
    expect(groups[1]?.items.map((i) => i.to)).toEqual(["/audit"]);
    expect(visibleGroups(null)).toEqual([]);
  });

  it("knows which group the current page belongs to", () => {
    const tools = NAV_GROUPS.find((g) => g.label === "Tools");
    if (!tools) throw new Error("no tools group");
    expect(groupIsActive(tools, "/precon-ev")).toBe(true);
    expect(groupIsActive(tools, "/precon-ev/774134")).toBe(true);
    expect(groupIsActive(tools, "/precon-evil")).toBe(false);
    expect(groupIsActive(tools, "/users")).toBe(false);
  });

  it("describes every section for the start page", () => {
    for (const item of NAV_GROUPS.flatMap((g) => g.items)) {
      expect(item.description.length, item.to).toBeGreaterThan(10);
    }
  });
});
