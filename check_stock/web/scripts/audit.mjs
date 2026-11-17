// `npm run audit:check`: `npm audit` with reviewed exceptions (rules in
// auditCheck.mjs, exceptions in audit-exceptions.json). Used by the
// quality script and CI instead of a plain `npm audit`.

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { advisories, judge, THRESHOLD } from "./auditCheck.mjs";

const here = fileURLToPath(new URL("..", import.meta.url));

function fail(message) {
  console.error(`npm audit check failed: ${message}`);
  process.exit(1);
}

let file;
try {
  file = JSON.parse(readFileSync(`${here}audit-exceptions.json`, "utf8"));
} catch (e) {
  fail(`audit-exceptions.json cannot be read (${e.message})`);
}

const run = spawnSync("npm", ["audit", "--json"], {
  cwd: here,
  encoding: "utf8",
  maxBuffer: 64 * 1024 * 1024,
  shell: process.platform === "win32",
});
let report;
try {
  report = JSON.parse(run.stdout);
} catch {
  fail(
    `npm audit gave no readable report (exit ${run.status}): ${(run.stderr || run.stdout || "").trim().slice(0, 300)}`,
  );
}

let found;
try {
  found = advisories(report);
} catch (e) {
  fail(e.message);
}

const today = new Date().toISOString().slice(0, 10);
const result = judge(found, file, today);
const line = (a) => `${a.id} ${a.package} (${a.severity}${a.range ? `, ${a.range}` : ""}): ${a.title}`;

for (const a of result.low) console.log(`note: below ${THRESHOLD}: ${line(a)}`);
for (const a of result.excepted) console.log(`accepted until ${a.until}: ${line(a)}`);
for (const a of result.blocking) console.error(`VULNERABLE: ${line(a)}`);
for (const p of result.problems) console.error(`EXCEPTION: ${p}`);

if (result.blocking.length > 0 || result.problems.length > 0) process.exit(1);
console.log(
  result.excepted.length > 0
    ? `npm audit: clean apart from ${result.excepted.length} accepted advisory (see audit-exceptions.json)`
    : "npm audit: clean",
);
