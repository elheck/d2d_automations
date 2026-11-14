import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { catalog } from "../test/fixtures";
import { PermissionPicker, toggle, withDependents, withRequirements } from "./PermissionPicker";

function Harness({ initial = [] as string[] }) {
  const [value, setValue] = useState(initial);
  return (
    <>
      <PermissionPicker catalog={catalog} value={value} onChange={setValue} />
      <output data-testid="value">{value.join(",")}</output>
    </>
  );
}

describe("permission rules", () => {
  it("follows requirements and dependents", () => {
    expect([...withRequirements(catalog, "repricing.export")]).toEqual(["repricing.export", "repricing.view"]);
    expect([...withDependents(catalog, "repricing.view")].sort()).toEqual([
      "repricing.edit_rules",
      "repricing.export",
      "repricing.view",
    ]);
  });

  it("keeps the selection in catalog order and consistent", () => {
    expect(toggle(catalog, [], "inventory.discard", true)).toEqual(["search.use", "inventory.discard"]);
    expect(toggle(catalog, ["search.use", "inventory.discard", "audit.view"], "search.use", false)).toEqual([
      "audit.view",
    ]);
  });
});

describe("PermissionPicker", () => {
  it("shows every permission with its description, grouped", () => {
    render(<Harness />);
    for (const group of ["Administration", "Daily work", "Inventory", "Pricing"]) {
      expect(screen.getByRole("group", { name: group })).toBeInTheDocument();
    }
    const pricing = screen.getByRole("group", { name: "Pricing" });
    expect(within(pricing).getAllByRole("checkbox")).toHaveLength(3);
    for (const p of catalog) {
      const box = screen.getByRole("checkbox", { name: p.label });
      expect(box).toHaveAccessibleDescription(expect.stringContaining(p.description));
    }
    expect(screen.getByRole("checkbox", { name: "Export new prices" })).toHaveAccessibleDescription(
      expect.stringContaining("Needs: View repricing"),
    );
  });

  it("ticks what a permission needs and unticks what depends on it", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("checkbox", { name: "Export new prices" }));
    expect(screen.getByTestId("value")).toHaveTextContent("repricing.view,repricing.export");
    await user.click(screen.getByRole("checkbox", { name: "Edit repricing rules" }));
    await user.click(screen.getByRole("checkbox", { name: "View repricing" }));
    expect(screen.getByTestId("value")).toHaveTextContent(/^$/);
  });

  it("offers all and none", async () => {
    const user = userEvent.setup();
    render(<Harness initial={["search.use"]} />);
    expect(screen.getByText(`1 of ${catalog.length} granted`)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "All" }));
    expect(screen.getByText(`${catalog.length} of ${catalog.length} granted`)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "None" }));
    expect(screen.getByText(`0 of ${catalog.length} granted`)).toBeInTheDocument();
  });
});
