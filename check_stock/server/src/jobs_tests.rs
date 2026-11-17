use super::*;
use chrono::TimeZone;

fn t(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, second).unwrap()
}

#[test]
fn a_run_reports_stage_progress_and_outcome() {
    let job = Arc::new(Job::default());
    assert_eq!(job.status(), JobStatus::Idle);
    let run = job.start("Starting", t(0)).unwrap();
    assert!(job.status().is_running());
    run.progress("Decklists", 3, 10);
    assert_eq!(
        job.status(),
        JobStatus::Running {
            started_at: "2026-11-15T12:00:00Z".to_string(),
            stage: "Decklists".to_string(),
            done: Some(3),
            total: Some(10),
        }
    );
    run.stage("Prices");
    assert!(matches!(
        job.status(),
        JobStatus::Running { done: None, total: None, ref stage, .. } if stage == "Prices"
    ));
    run.finish(t(5));
    assert_eq!(
        job.status(),
        JobStatus::Finished {
            at: "2026-11-15T12:00:05Z".to_string()
        }
    );
}

#[test]
fn only_one_run_at_a_time() {
    let job = Arc::new(Job::default());
    let run = job.start("A", t(0)).unwrap();
    assert!(job.start("B", t(1)).is_none());
    run.fail("broken", t(2));
    assert!(matches!(job.status(), JobStatus::Failed { ref error, .. } if error == "broken"));
    // After the end a new run may start.
    let again = job.start("C", t(3)).unwrap();
    again.cancelled(t(4));
    assert!(matches!(job.status(), JobStatus::Cancelled { .. }));
}

#[test]
fn a_replaced_run_is_cancelled_and_silenced() {
    let job = Arc::new(Job::default());
    let old = job.start("old", t(0)).unwrap();
    let new = job.replace("new", t(1));
    assert!(old.is_cancelled());
    assert!(!old.is_current());
    assert!(!new.is_cancelled());
    old.progress("old progress", 1, 2);
    old.finish(t(2));
    assert!(matches!(job.status(), JobStatus::Running { ref stage, .. } if stage == "new"));
    new.finish(t(3));
    assert!(matches!(job.status(), JobStatus::Finished { .. }));
}

#[test]
fn cancel_sets_the_flag_of_the_running_run() {
    let job = Arc::new(Job::default());
    let run = job.start("A", t(0)).unwrap();
    job.cancel();
    assert!(run.is_cancelled());
    assert!(run.cancel_flag().load(Ordering::Relaxed));
    run.cancelled(t(1));
}

#[test]
fn a_run_dropped_without_outcome_counts_as_failed() {
    let job = Arc::new(Job::default());
    {
        let _run = job.start("A", t(0)).unwrap();
    }
    assert!(
        matches!(job.status(), JobStatus::Failed { ref error, .. } if error.contains("unexpectedly"))
    );
    // A dropped run that was superseded does not touch the new one.
    let old = job.start("B", t(1)).unwrap();
    let new = job.replace("C", t(2));
    drop(old);
    assert!(job.status().is_running());
    new.finish(t(3));
}
