//! Background jobs whose results all users share (e.g. the precon load).
//!
//! A [`Job`] is one kind of work with a status that pages poll: idle,
//! running (stage text and progress), finished, failed or cancelled. Only one
//! run of a kind is active at a time; [`Job::start`] returns `None` while one
//! runs. [`Job::replace`] cancels the running one and takes over, and the old
//! run's [`JobRun`] can then no longer change the status. A run that ends
//! without reporting (a panic) is marked as failed when its handle is dropped.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use ts_rs::TS;

/// What a job is doing; times are RFC 3339 UTC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
#[ts(export)]
pub enum JobStatus {
    /// Never ran since the server started.
    Idle,
    Running {
        started_at: String,
        /// What it is doing right now, for people.
        stage: String,
        /// Items done out of `total`, when the stage counts something.
        done: Option<u64>,
        total: Option<u64>,
    },
    Finished {
        at: String,
    },
    Failed {
        at: String,
        error: String,
    },
    Cancelled {
        at: String,
    },
}

impl JobStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, JobStatus::Running { .. })
    }
}

struct State {
    status: JobStatus,
    /// Increases with every run; a run only reports while it is current.
    generation: u64,
    cancel: Arc<AtomicBool>,
}

/// One kind of background work.
pub struct Job {
    state: Mutex<State>,
}

impl Default for Job {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                status: JobStatus::Idle,
                generation: 0,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
        }
    }
}

fn time(at: DateTime<Utc>) -> String {
    crate::migrations::format_utc(at)
}

impl Job {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn status(&self) -> JobStatus {
        self.lock().status.clone()
    }

    /// Starts a run unless one is running.
    pub fn start(self: &Arc<Self>, stage: &str, now: DateTime<Utc>) -> Option<JobRun> {
        let mut state = self.lock();
        if state.status.is_running() {
            return None;
        }
        Some(self.begin(&mut state, stage, now))
    }

    /// Starts a run, cancelling the one that is running (if any).
    pub fn replace(self: &Arc<Self>, stage: &str, now: DateTime<Utc>) -> JobRun {
        let mut state = self.lock();
        state.cancel.store(true, Ordering::Relaxed);
        self.begin(&mut state, stage, now)
    }

    /// Asks the running run (if any) to stop at its next check.
    pub fn cancel(&self) {
        self.lock().cancel.store(true, Ordering::Relaxed);
    }

    fn begin(self: &Arc<Self>, state: &mut State, stage: &str, now: DateTime<Utc>) -> JobRun {
        state.generation += 1;
        state.cancel = Arc::new(AtomicBool::new(false));
        state.status = JobStatus::Running {
            started_at: time(now),
            stage: stage.to_string(),
            done: None,
            total: None,
        };
        JobRun {
            job: self.clone(),
            generation: state.generation,
            cancel: state.cancel.clone(),
            ended: false,
        }
    }
}

/// The handle of one run: reports progress and the outcome.
pub struct JobRun {
    job: Arc<Job>,
    generation: u64,
    cancel: Arc<AtomicBool>,
    ended: bool,
}

impl JobRun {
    /// Set when the run should stop (replaced, cancelled, server shutdown).
    pub fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Whether this run is still the job's current one.
    pub fn is_current(&self) -> bool {
        self.job.lock().generation == self.generation
    }

    fn update(&self, f: impl FnOnce(&mut JobStatus)) {
        let mut state = self.job.lock();
        if state.generation == self.generation {
            f(&mut state.status);
        }
    }

    /// New stage text; clears the progress counts.
    pub fn stage(&self, text: &str) {
        self.update(|status| {
            if let JobStatus::Running {
                stage, done, total, ..
            } = status
            {
                *stage = text.to_string();
                *done = None;
                *total = None;
            }
        });
    }

    pub fn progress(&self, text: &str, done_now: u64, total_now: u64) {
        self.update(|status| {
            if let JobStatus::Running {
                stage, done, total, ..
            } = status
            {
                *stage = text.to_string();
                *done = Some(done_now);
                *total = Some(total_now);
            }
        });
    }

    pub fn finish(mut self, now: DateTime<Utc>) {
        self.ended = true;
        self.update(|status| *status = JobStatus::Finished { at: time(now) });
    }

    pub fn fail(mut self, error: &str, now: DateTime<Utc>) {
        self.ended = true;
        self.update(|status| {
            *status = JobStatus::Failed {
                at: time(now),
                error: error.to_string(),
            }
        });
    }

    pub fn cancelled(mut self, now: DateTime<Utc>) {
        self.ended = true;
        self.update(|status| *status = JobStatus::Cancelled { at: time(now) });
    }
}

impl Drop for JobRun {
    fn drop(&mut self) {
        if !self.ended {
            self.update(|status| {
                *status = JobStatus::Failed {
                    at: time(Utc::now()),
                    error: "The job stopped unexpectedly".to_string(),
                }
            });
        }
    }
}

/// The part of a loader or tracker error that is meant for people: `"Catalog fetch
/// failed: error sending request for url (http://…)"` → `"Catalog fetch
/// failed."`. The full text (addresses, causes) only goes to the log.
pub fn public_message(error: &str) -> String {
    let head = error
        .split_once(": ")
        .map_or(error, |(head, _)| head)
        .trim();
    let head = head.trim_end_matches('.');
    if head.is_empty() {
        "The load failed.".to_string()
    } else {
        format!("{head}.")
    }
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod tests;
