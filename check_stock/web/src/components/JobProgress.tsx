// The stage and progress of a running server job (e.g. a precon or market
// data load); nothing when the job is not running.

import type { JobStatus } from "../api/types";
import { jobText } from "../pages/preconEv";

export function JobProgress({ status }: { status: JobStatus }) {
  if (status.state !== "running") return null;
  return (
    <span className="job-progress" role="status">
      <span className="spinner" aria-hidden="true" />
      <span>{jobText(status)}</span>
      {status.done !== null && status.total !== null && status.total > 0 && (
        <progress value={status.done} max={status.total} />
      )}
    </span>
  );
}
