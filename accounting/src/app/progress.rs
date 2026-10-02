//! Progress reporting for background invoice processing.
//!
//! Invoices are created on the Tokio runtime so the GUI stays responsive.
//! The worker sends [`ProgressEvent`]s over a channel; the GUI folds them
//! into a [`ProgressTracker`] every frame.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::models::InvoiceCreationResult;

/// What the worker is currently doing for the item in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressStep {
    CreatingInvoice,
    RunningWorkflow,
}

impl ProgressStep {
    pub fn label(self, dry_run: bool) -> &'static str {
        match (self, dry_run) {
            (ProgressStep::CreatingInvoice, false) => "Creating invoice",
            (ProgressStep::CreatingInvoice, true) => "Simulating invoice",
            (ProgressStep::RunningWorkflow, false) => {
                "Running workflow (finalize / enshrine / book)"
            }
            (ProgressStep::RunningWorkflow, true) => "Simulating workflow",
        }
    }
}

/// Message sent from the background worker to the GUI.
#[derive(Debug)]
pub enum ProgressEvent {
    /// Work on a new item began. `label` identifies it to the user.
    ItemStarted { label: String },
    /// The invoice exists; post-creation workflow steps are running.
    WorkflowStarted,
    /// An item finished (successfully or not).
    ItemFinished(Box<InvoiceCreationResult>),
}

/// Outcome of draining the channel once.
#[derive(Debug, Default)]
pub struct ProgressPoll {
    /// Results that completed since the last poll, in order.
    pub results: Vec<InvoiceCreationResult>,
    /// True once the worker has exited and all events were consumed.
    pub done: bool,
}

/// GUI-side view of a running invoice job.
pub struct ProgressTracker {
    rx: Receiver<ProgressEvent>,
    started_at: Instant,
    pub total: usize,
    pub completed: usize,
    pub success_count: usize,
    pub error_count: usize,
    pub current_item: Option<String>,
    pub step: ProgressStep,
    pub dry_run: bool,
}

impl ProgressTracker {
    pub fn new(rx: Receiver<ProgressEvent>, total: usize, dry_run: bool) -> Self {
        Self {
            rx,
            started_at: Instant::now(),
            total,
            completed: 0,
            success_count: 0,
            error_count: 0,
            current_item: None,
            step: ProgressStep::CreatingInvoice,
            dry_run,
        }
    }

    /// Applies a single event, returning the result if an item finished.
    pub fn apply(&mut self, event: ProgressEvent) -> Option<InvoiceCreationResult> {
        match event {
            ProgressEvent::ItemStarted { label } => {
                self.current_item = Some(label);
                self.step = ProgressStep::CreatingInvoice;
                None
            }
            ProgressEvent::WorkflowStarted => {
                self.step = ProgressStep::RunningWorkflow;
                None
            }
            ProgressEvent::ItemFinished(result) => {
                self.completed += 1;
                if result.error.is_none() {
                    self.success_count += 1;
                } else {
                    self.error_count += 1;
                }
                Some(*result)
            }
        }
    }

    /// Drains all pending events without blocking.
    pub fn poll(&mut self) -> ProgressPoll {
        let mut poll = ProgressPoll::default();
        loop {
            match self.rx.try_recv() {
                Ok(event) => poll.results.extend(self.apply(event)),
                Err(TryRecvError::Empty) => break,
                // The worker dropped its sender: it finished (or died).
                Err(TryRecvError::Disconnected) => {
                    poll.done = true;
                    break;
                }
            }
        }
        poll
    }

    /// Completed fraction in `0.0..=1.0`.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        (self.completed as f32 / self.total as f32).min(1.0)
    }

    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Estimated remaining time, extrapolated from the average time per finished item.
    pub fn eta(&self) -> Option<Duration> {
        estimate_remaining(self.elapsed(), self.completed, self.total)
    }
}

/// Extrapolates the remaining time from the average duration per completed item.
/// Returns `None` until at least one item completed.
pub fn estimate_remaining(elapsed: Duration, completed: usize, total: usize) -> Option<Duration> {
    if completed == 0 || completed > total {
        return None;
    }
    let per_item = elapsed.as_secs_f64() / completed as f64;
    Some(Duration::from_secs_f64(
        per_item * (total - completed) as f64,
    ))
}

/// Formats a duration as `m:ss` (or `h:mm:ss` beyond an hour).
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
