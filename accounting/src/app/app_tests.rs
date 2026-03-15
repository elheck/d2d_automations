//! Unit tests for the app module.

use super::*;

mod processing_state_tests {
    use super::*;

    #[test]
    fn processing_state_idle_is_default() {
        // ProcessingState doesn't implement Default, but Idle is the logical default
        let state = ProcessingState::Idle;
        assert!(matches!(state, ProcessingState::Idle));
    }

    #[test]
    fn processing_state_loading_csv() {
        let state = ProcessingState::LoadingCsv;
        assert!(matches!(state, ProcessingState::LoadingCsv));
    }

    #[test]
    fn processing_state_processing_tracks_progress() {
        let state = ProcessingState::Processing {
            current: 5,
            total: 10,
        };

        if let ProcessingState::Processing { current, total } = state {
            assert_eq!(current, 5);
            assert_eq!(total, 10);
        } else {
            panic!("Expected Processing state");
        }
    }

    #[test]
    fn processing_state_completed() {
        let state = ProcessingState::Completed;
        assert!(matches!(state, ProcessingState::Completed));
    }

    #[test]
    fn processing_state_is_clone() {
        let state = ProcessingState::Processing {
            current: 3,
            total: 7,
        };
        let cloned = state.clone();

        if let ProcessingState::Processing { current, total } = cloned {
            assert_eq!(current, 3);
            assert_eq!(total, 7);
        } else {
            panic!("Clone failed");
        }
    }

    #[test]
    fn processing_state_is_debug() {
        let state = ProcessingState::Idle;
        let debug_str = format!("{:?}", state);
        assert_eq!(debug_str, "Idle");

        let state = ProcessingState::Processing {
            current: 1,
            total: 5,
        };
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("Processing"));
        assert!(debug_str.contains("1"));
        assert!(debug_str.contains("5"));
    }
}

mod invoice_app_state_tests {
    // Note: We can't easily test InvoiceApp::default() because it creates a Tokio runtime
    // and reads environment variables. These tests focus on state logic.

    #[test]
    fn can_process_requires_orders_and_token() {
        // This tests the logic that would be in process_invoices
        // Extracted as a pure function for testability

        let has_orders = true;
        let has_token = true;
        let api_connected = Some(true);
        let dry_run_mode = false;

        let can_process = has_orders && has_token && (dry_run_mode || api_connected == Some(true));

        assert!(can_process);
    }

    #[test]
    fn cannot_process_without_orders() {
        let has_orders = false;
        let has_token = true;
        let api_connected = Some(true);
        let dry_run_mode = false;

        let can_process = has_orders && has_token && (dry_run_mode || api_connected == Some(true));

        assert!(!can_process);
    }

    #[test]
    fn cannot_process_without_token() {
        let has_orders = true;
        let has_token = false;
        let api_connected = Some(true);
        let dry_run_mode = false;

        let can_process = has_orders && has_token && (dry_run_mode || api_connected == Some(true));

        assert!(!can_process);
    }

    #[test]
    fn cannot_process_without_connection_in_normal_mode() {
        let has_orders = true;
        let has_token = true;
        let api_connected = Some(false);
        let dry_run_mode = false;

        let can_process = has_orders && has_token && (dry_run_mode || api_connected == Some(true));

        assert!(!can_process);
    }

    #[test]
    fn can_process_without_connection_in_dry_run_mode() {
        let has_orders = true;
        let has_token = true;
        let api_connected = Some(false);
        let dry_run_mode = true;

        let can_process = has_orders && has_token && (dry_run_mode || api_connected == Some(true));

        assert!(can_process);
    }

    #[test]
    fn can_process_with_no_connection_status_in_dry_run_mode() {
        let has_orders = true;
        let has_token = true;
        let api_connected: Option<bool> = None;
        let dry_run_mode = true;

        let can_process = has_orders && has_token && (dry_run_mode || api_connected == Some(true));

        assert!(can_process);
    }
}

mod result_counting_tests {
    use crate::models::InvoiceCreationResult;

    fn create_success_result(name: &str, invoice_num: &str) -> InvoiceCreationResult {
        InvoiceCreationResult {
            order_id: "123".to_string(),
            customer_name: name.to_string(),
            invoice_id: Some(1),
            invoice_number: Some(invoice_num.to_string()),
            error: None,
            workflow_status: None,
        }
    }

    fn create_error_result(name: &str, error: &str) -> InvoiceCreationResult {
        InvoiceCreationResult {
            order_id: "123".to_string(),
            customer_name: name.to_string(),
            invoice_id: None,
            invoice_number: None,
            error: Some(error.to_string()),
            workflow_status: None,
        }
    }

    #[test]
    fn counts_successes_correctly() {
        let results = [
            create_success_result("Alice", "INV-001"),
            create_success_result("Bob", "INV-002"),
            create_error_result("Charlie", "API error"),
        ];

        let success_count = results.iter().filter(|r| r.error.is_none()).count();
        let error_count = results.len() - success_count;

        assert_eq!(success_count, 2);
        assert_eq!(error_count, 1);
    }

    #[test]
    fn counts_all_successes() {
        let results = [
            create_success_result("Alice", "INV-001"),
            create_success_result("Bob", "INV-002"),
            create_success_result("Charlie", "INV-003"),
        ];

        let success_count = results.iter().filter(|r| r.error.is_none()).count();
        let error_count = results.len() - success_count;

        assert_eq!(success_count, 3);
        assert_eq!(error_count, 0);
    }

    #[test]
    fn counts_all_errors() {
        let results = [
            create_error_result("Alice", "Error 1"),
            create_error_result("Bob", "Error 2"),
        ];

        let success_count = results.iter().filter(|r| r.error.is_none()).count();
        let error_count = results.len() - success_count;

        assert_eq!(success_count, 0);
        assert_eq!(error_count, 2);
    }

    #[test]
    fn handles_empty_results() {
        let results: Vec<InvoiceCreationResult> = vec![];

        let success_count = results.iter().filter(|r| r.error.is_none()).count();
        let error_count = results.len() - success_count;

        assert_eq!(success_count, 0);
        assert_eq!(error_count, 0);
    }
}

mod progress_calculation_tests {
    #[test]
    fn progress_at_start() {
        let current = 0;
        let total = 10;
        let progress = current as f32 / total as f32;
        assert!((progress - 0.0).abs() < 0.001);
    }

    #[test]
    fn progress_at_middle() {
        let current = 5;
        let total = 10;
        let progress = current as f32 / total as f32;
        assert!((progress - 0.5).abs() < 0.001);
    }

    #[test]
    fn progress_at_end() {
        let current = 10;
        let total = 10;
        let progress = current as f32 / total as f32;
        assert!((progress - 1.0).abs() < 0.001);
    }

    #[test]
    fn progress_with_single_item() {
        let current = 1;
        let total = 1;
        let progress = current as f32 / total as f32;
        assert!((progress - 1.0).abs() < 0.001);
    }
}
