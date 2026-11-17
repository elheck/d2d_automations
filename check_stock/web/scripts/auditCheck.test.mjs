import { describe, expect, it } from "vitest";
import { advisories, exceptionProblems, judge, MAX_EXCEPTION_DAYS } from "./auditCheck.mjs";

const ID = "GHSA-fc5q-39xc-vjpf";

function report(...entries) {
  const vulnerabilities = {};
  for (const [name, severity, id] of entries) {
    vulnerabilities[name] = {
      name,
      severity,
      via: [
        {
          source: 1,
          name,
          title: `${name} is bad`,
          url: `https://github.com/advisories/${id}`,
          severity,
          range: "<=4.0.7",
        },
      ],
    };
  }
  // A package that only depends on a vulnerable one (string "via").
  vulnerabilities.vite = { name: "vite", severity: "high", via: [entries[0]?.[0] ?? "x"] };
  return { auditReportVersion: 2, vulnerabilities };
}

const exception = (overrides = {}) => ({
  advisory: ID,
  package: "picomatch",
  reason: "Only build tools use it, with our own patterns; no fixed release yet.",
  added: "2026-11-17",
  until: "2027-02-15",
  ...overrides,
});

describe("npm audit check", () => {
  it("reads each advisory once, from the vulnerable package", () => {
    const found = advisories(report(["picomatch", "high", ID]));
    expect(found).toEqual([
      { id: ID, package: "picomatch", severity: "high", title: "picomatch is bad", range: "<=4.0.7" },
    ]);
    expect(advisories({ auditReportVersion: 2, vulnerabilities: {} })).toEqual([]);
  });

  it("refuses reports it cannot read", () => {
    expect(() => advisories(undefined)).toThrow("no report");
    expect(() => advisories({ error: { code: "ENOTFOUND", summary: "network down" } })).toThrow("network down");
    expect(() => advisories({ advisories: {} })).toThrow("unknown format");
  });

  it("passes a listed advisory until its end date, and nothing else", () => {
    const found = advisories(report(["picomatch", "high", ID], ["lodash", "critical", "GHSA-xxxx-xxxx-xxxx"]));
    const result = judge(found, { exceptions: [exception()] }, "2026-11-17");
    expect(result.excepted.map((a) => a.package)).toEqual(["picomatch"]);
    expect(result.blocking.map((a) => a.package)).toEqual(["lodash"]);
    expect(result.problems).toEqual([]);
  });

  it("matches advisory and package exactly", () => {
    const found = advisories(report(["minimatch", "high", ID]));
    const result = judge(found, { exceptions: [exception()] }, "2026-11-17");
    expect(result.blocking.map((a) => a.package)).toEqual(["minimatch"]);
    expect(result.problems).toEqual([`${ID} (picomatch) is no longer reported: remove its exception`]);
  });

  it("fails once an exception ended", () => {
    const found = advisories(report(["picomatch", "high", ID]));
    const result = judge(found, { exceptions: [exception()] }, "2027-02-16");
    expect(result.blocking.map((a) => a.package)).toEqual(["picomatch"]);
    expect(result.problems).toEqual([`the exception for ${ID} (picomatch) ended on 2027-02-15: look at it again`]);
    expect(judge(found, { exceptions: [exception()] }, "2027-02-15").blocking).toEqual([]);
  });

  it("asks to remove an exception once the advisory is fixed", () => {
    const result = judge([], { exceptions: [exception()] }, "2026-12-01");
    expect(result.problems).toEqual([`${ID} (picomatch) is no longer reported: remove its exception`]);
  });

  it("only notes advisories below moderate", () => {
    const found = advisories(report(["tiny", "low", "GHSA-2222-3333-4444"]));
    const result = judge(found, { exceptions: [] }, "2026-11-17");
    expect(result.low.map((a) => a.package)).toEqual(["tiny"]);
    expect(result.blocking).toEqual([]);
    expect(
      judge(advisories(report(["mid", "moderate", "GHSA-2222-3333-4444"])), { exceptions: [] }, "2026-11-17").blocking,
    ).toHaveLength(1);
  });

  it("refuses vague, long or duplicate exceptions", () => {
    expect(exceptionProblems({})).toEqual(["audit-exceptions.json needs an `exceptions` list"]);
    expect(exceptionProblems({ exceptions: [exception()] })).toEqual([]);
    const bad = (overrides) => exceptionProblems({ exceptions: [exception(overrides)] });
    expect(bad({ advisory: "GHSA-*" })).toEqual([`exception 1: "advisory" must be one GHSA id`]);
    expect(bad({ advisory: "CVE-2026-1" })).toHaveLength(1);
    expect(bad({ package: "" })).toEqual([`exception 1: "package" must name the vulnerable package`]);
    expect(bad({ reason: "trust me" })).toEqual([`exception 1: "reason" must explain why the risk is accepted`]);
    expect(bad({ until: "someday" })).toEqual([`exception 1: "added" and "until" must be dates (YYYY-MM-DD)`]);
    expect(bad({ until: "2027-12-31" })).toEqual([
      `exception 1: "until" must be 0–${MAX_EXCEPTION_DAYS} days after "added"`,
    ]);
    expect(bad({ until: "2026-11-16" })).toHaveLength(1);
    expect(exceptionProblems({ exceptions: [exception(), exception()] })).toEqual([
      `exception 2: ${ID} picomatch is listed twice`,
    ]);
    // A bad file excepts nothing.
    const found = advisories(report(["picomatch", "high", ID]));
    expect(judge(found, { exceptions: [exception({ reason: "x" })] }, "2026-11-17").blocking).toHaveLength(1);
  });
});
