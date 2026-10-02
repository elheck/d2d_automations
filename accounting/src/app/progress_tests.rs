//! Unit tests for background progress tracking.

use super::*;
use std::sync::mpsc;

fn result(name: &str, error: Option<&str>) -> InvoiceCreationResult {
    InvoiceCreationResult {
        order_id: "1".to_string(),
        customer_name: name.to_string(),
        invoice_id: error.is_none().then_some(1),
        invoice_number: error.is_none().then(|| "RE-1".to_string()),
        error: error.map(str::to_string),
        workflow_status: None,
    }
}

fn finished(name: &str, error: Option<&str>) -> ProgressEvent {
    ProgressEvent::ItemFinished(Box::new(result(name, error)))
}

#[test]
fn new_tracker_starts_empty() {
    let (_tx, rx) = mpsc::channel();
    let tracker = ProgressTracker::new(rx, 4, false);

    assert_eq!(tracker.total, 4);
    assert_eq!(tracker.completed, 0);
    assert_eq!(tracker.fraction(), 0.0);
    assert!(tracker.current_item.is_none());
    assert!(tracker.eta().is_none());
}

#[test]
fn apply_tracks_item_and_step() {
    let (_tx, rx) = mpsc::channel();
    let mut tracker = ProgressTracker::new(rx, 2, false);

    assert!(tracker
        .apply(ProgressEvent::ItemStarted {
            label: "Alice (123)".to_string()
        })
        .is_none());
    assert_eq!(tracker.current_item.as_deref(), Some("Alice (123)"));
    assert_eq!(tracker.step, ProgressStep::CreatingInvoice);

    assert!(tracker.apply(ProgressEvent::WorkflowStarted).is_none());
    assert_eq!(tracker.step, ProgressStep::RunningWorkflow);

    // The next item resets the step.
    tracker.apply(ProgressEvent::ItemStarted {
        label: "Bob (456)".to_string(),
    });
    assert_eq!(tracker.step, ProgressStep::CreatingInvoice);
}

#[test]
fn apply_counts_successes_and_errors() {
    let (_tx, rx) = mpsc::channel();
    let mut tracker = ProgressTracker::new(rx, 4, false);

    let returned = tracker.apply(finished("Alice", None)).unwrap();
    assert_eq!(returned.customer_name, "Alice");
    tracker.apply(finished("Bob", Some("boom")));

    assert_eq!(tracker.completed, 2);
    assert_eq!(tracker.success_count, 1);
    assert_eq!(tracker.error_count, 1);
    assert_eq!(tracker.fraction(), 0.5);
}

#[test]
fn poll_drains_events_in_order_and_detects_completion() {
    let (tx, rx) = mpsc::channel();
    let mut tracker = ProgressTracker::new(rx, 2, true);

    tx.send(ProgressEvent::ItemStarted {
        label: "Alice".to_string(),
    })
    .unwrap();
    tx.send(finished("Alice", None)).unwrap();

    let poll = tracker.poll();
    assert_eq!(poll.results.len(), 1);
    assert!(!poll.done);

    tx.send(finished("Bob", None)).unwrap();
    drop(tx);

    let poll = tracker.poll();
    assert_eq!(poll.results.len(), 1);
    assert_eq!(poll.results[0].customer_name, "Bob");
    assert!(poll.done);
    assert_eq!(tracker.fraction(), 1.0);
}

#[test]
fn poll_without_events_is_not_done() {
    let (_tx, rx) = mpsc::channel();
    let mut tracker = ProgressTracker::new(rx, 1, false);

    let poll = tracker.poll();
    assert!(poll.results.is_empty());
    assert!(!poll.done);
}

#[test]
fn fraction_handles_zero_total() {
    let (_tx, rx) = mpsc::channel();
    let tracker = ProgressTracker::new(rx, 0, false);
    assert_eq!(tracker.fraction(), 0.0);
}

#[test]
fn estimate_remaining_extrapolates_average() {
    let eta = estimate_remaining(Duration::from_secs(10), 2, 6).unwrap();
    assert_eq!(eta.as_secs(), 20);
}

#[test]
fn estimate_remaining_needs_a_completed_item() {
    assert!(estimate_remaining(Duration::from_secs(10), 0, 6).is_none());
}

#[test]
fn estimate_remaining_is_zero_when_done() {
    let eta = estimate_remaining(Duration::from_secs(10), 6, 6).unwrap();
    assert_eq!(eta, Duration::ZERO);
}

#[test]
fn format_duration_minutes_and_hours() {
    assert_eq!(format_duration(Duration::from_secs(0)), "0:00");
    assert_eq!(format_duration(Duration::from_secs(75)), "1:15");
    assert_eq!(format_duration(Duration::from_secs(3725)), "1:02:05");
}

#[test]
fn step_labels_differ_for_dry_run() {
    assert_eq!(
        ProgressStep::CreatingInvoice.label(false),
        "Creating invoice"
    );
    assert_eq!(
        ProgressStep::CreatingInvoice.label(true),
        "Simulating invoice"
    );
    assert_ne!(
        ProgressStep::RunningWorkflow.label(false),
        ProgressStep::RunningWorkflow.label(true)
    );
}
