// The rules of `npm run audit:check` (tested in auditCheck.test.mjs).
//
// `npm audit` must stay clean: every advisory of moderate or higher severity
// fails the check, unless audit-exceptions.json names exactly that advisory
// for exactly that package, with a reason and an end date. An exception is
// itself a failure once it ends, or once the advisory is no longer reported
// (then it must be removed), so none outlives its reason.

/** Severities in order; advisories below the threshold only show as notes. */
export const SEVERITIES = ["info", "low", "moderate", "high", "critical"];
export const THRESHOLD = "moderate";
/** The longest an exception may run before it has to be looked at again. */
export const MAX_EXCEPTION_DAYS = 120;

const GHSA = /^GHSA(-[23456789cfghjmpqrvwx]{4}){3}$/;
const DAY = /^\d{4}-\d{2}-\d{2}$/;

const rank = (severity) => SEVERITIES.indexOf(severity);

/** The advisories of an `npm audit --json` report (version 2), one per advisory and package. */
export function advisories(report) {
  if (!report || typeof report !== "object") throw new Error("npm audit gave no report");
  if (report.error) {
    throw new Error(`npm audit failed: ${report.error.summary ?? report.error.code ?? "unknown error"}`);
  }
  if (!report.vulnerabilities || typeof report.vulnerabilities !== "object") {
    throw new Error("the npm audit report has no list of vulnerabilities (unknown format)");
  }
  const found = new Map();
  for (const vulnerability of Object.values(report.vulnerabilities)) {
    for (const via of vulnerability.via ?? []) {
      // Strings name the dependency that carries the advisory; the advisory
      // itself is listed (as an object) under the vulnerable package.
      if (!via || typeof via !== "object") continue;
      const id =
        String(via.url ?? "")
          .split("/")
          .pop() || `npm-${via.source}`;
      const key = `${id} ${via.name}`;
      if (!found.has(key)) {
        found.set(key, { id, package: via.name, severity: via.severity, title: via.title, range: via.range });
      }
    }
  }
  return [...found.values()];
}

function daysBetween(from, to) {
  return Math.round((Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / 86_400_000);
}

/** What is wrong with the exceptions file itself (empty: nothing). */
export function exceptionProblems(file) {
  if (!file || !Array.isArray(file.exceptions)) return ["audit-exceptions.json needs an `exceptions` list"];
  const problems = [];
  const seen = new Set();
  file.exceptions.forEach((e, i) => {
    const at = `exception ${i + 1}`;
    if (!e || typeof e !== "object") {
      problems.push(`${at} is not an object`);
      return;
    }
    if (typeof e.advisory !== "string" || !GHSA.test(e.advisory))
      problems.push(`${at}: "advisory" must be one GHSA id`);
    if (typeof e.package !== "string" || !/^[a-z0-9@/._-]+$/.test(e.package)) {
      problems.push(`${at}: "package" must name the vulnerable package`);
    }
    if (typeof e.reason !== "string" || e.reason.trim().length < 40) {
      problems.push(`${at}: "reason" must explain why the risk is accepted`);
    }
    if (typeof e.added !== "string" || !DAY.test(e.added) || typeof e.until !== "string" || !DAY.test(e.until)) {
      problems.push(`${at}: "added" and "until" must be dates (YYYY-MM-DD)`);
    } else {
      const days = daysBetween(e.added, e.until);
      if (!(days >= 0 && days <= MAX_EXCEPTION_DAYS)) {
        problems.push(`${at}: "until" must be 0–${MAX_EXCEPTION_DAYS} days after "added"`);
      }
    }
    const key = `${e.advisory} ${e.package}`;
    if (seen.has(key)) problems.push(`${at}: ${key} is listed twice`);
    seen.add(key);
  });
  return problems;
}

/**
 * Judges the advisories against the exceptions on `today` (YYYY-MM-DD):
 * `blocking` fail the check, `excepted` pass with a note, `low` are below
 * the threshold, `problems` (expired, stale or malformed exceptions) fail.
 */
export function judge(found, file, today) {
  const problems = exceptionProblems(file);
  const exceptions = problems.length === 0 ? file.exceptions : [];
  const result = { blocking: [], excepted: [], low: [], problems };
  for (const a of found) {
    if (rank(a.severity) < rank(THRESHOLD)) {
      result.low.push(a);
      continue;
    }
    const exception = exceptions.find((e) => e.advisory === a.id && e.package === a.package);
    if (exception && today <= exception.until) result.excepted.push({ ...a, until: exception.until });
    else result.blocking.push(a);
  }
  for (const e of exceptions) {
    if (today > e.until) {
      problems.push(`the exception for ${e.advisory} (${e.package}) ended on ${e.until}: look at it again`);
    } else if (!found.some((a) => a.id === e.advisory && a.package === e.package)) {
      problems.push(`${e.advisory} (${e.package}) is no longer reported: remove its exception`);
    }
  }
  return result;
}
