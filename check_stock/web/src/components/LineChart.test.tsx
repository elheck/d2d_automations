import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { labelledDates, LineChart, linePath, niceStep, shortDate } from "./LineChart";

describe("line chart helpers", () => {
  it("picks round steps", () => {
    expect(niceStep(100)).toBe(25);
    expect(niceStep(37)).toBe(10);
    expect(niceStep(7)).toBe(2);
    expect(niceStep(0)).toBe(1);
    expect(niceStep(Number.NaN)).toBe(1);
  });

  it("breaks lines at missing values", () => {
    const x = (i: number) => i * 10;
    const y = (v: number) => 100 - v;
    expect(linePath([1, 2, null, 4, 5], x, y)).toBe("M0.0 99.0 L10.0 98.0 M30.0 96.0 L40.0 95.0");
    expect(linePath([null, null], x, y)).toBe("");
  });

  it("labels the first and last date and keeps labels apart", () => {
    expect(labelledDates(0, 3)).toEqual([]);
    expect(labelledDates(1, 3)).toEqual([0]);
    expect(labelledDates(10, 3)).toEqual([0, 3, 6, 9]);
    // 8 would sit right next to the last one (9): left out.
    expect(labelledDates(10, 4)).toEqual([0, 4, 9]);
    expect(labelledDates(53, 4).at(-1)).toBe(52);
    // Weekly labels every 2 weeks: the one a week before the last is left out.
    expect(labelledDates(26, 2).slice(-2)).toEqual([22, 25]);
  });

  it("writes short dates", () => {
    expect(shortDate("2026-03-02")).toBe("Mar 2");
    expect(shortDate("nonsense")).toBe("nonsense");
  });
});

describe("line chart", () => {
  const dates = ["2026-11-01", "2026-11-08", "2026-11-15"];
  const chart = () =>
    render(
      <LineChart
        label="Test chart"
        dates={dates}
        format={(v) => `${v.toFixed(2)} €`}
        series={[
          { id: "a", label: "EV", className: "chart-series--ev", values: [10, null, 30] },
          { id: "b", label: "Sealed", className: "chart-series--sealed", values: [20, 21, 22] },
        ]}
      />,
    );

  it("draws a line per series and lists the latest values", () => {
    const { container } = chart();
    expect(screen.getByRole("img", { name: "Test chart" })).toBeInTheDocument();
    expect(container.querySelectorAll("path.chart__line")).toHaveLength(2);
    // The lone points of the gapped series get a dot.
    expect(container.querySelectorAll(".chart-series--ev circle")).toHaveLength(2);
    expect(screen.getByText("EV: 30.00 €")).toBeInTheDocument();
    expect(screen.getByText("Sealed: 22.00 €")).toBeInTheDocument();
    const table = screen.getByRole("table", { name: "Test chart" });
    expect(within(table).getAllByRole("row")).toHaveLength(4);
    expect(within(table).getByRole("row", { name: /2026-11-08/ })).toHaveTextContent("—");
  });

  it("shows the values of the date under the pointer", () => {
    const { container } = chart();
    const hit = container.querySelector(".chart__hit") as SVGRectElement;
    hit.getBoundingClientRect = () => ({
      left: 0,
      width: 200,
      top: 0,
      height: 100,
      right: 200,
      bottom: 100,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });
    fireEvent.pointerMove(hit, { clientX: 100 });
    expect(screen.getByText("EV: —")).toBeInTheDocument();
    expect(screen.getByText("Sealed: 21.00 €")).toBeInTheDocument();
    expect(container.querySelector(".chart__cursor")).not.toBeNull();
    fireEvent.pointerLeave(hit);
    expect(screen.getByText("EV: 30.00 €")).toBeInTheDocument();
  });
});
