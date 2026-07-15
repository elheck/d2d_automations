//! Tests for web.

use super::*;
use crate::init_schema;
use rusqlite::Connection;
use tempfile::TempDir;

fn create_test_db() -> (Connection, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let conn = Connection::open(&db_path).unwrap();
    init_schema(&conn).unwrap();
    (conn, temp_dir)
}

#[test]
fn test_create_router() {
    let (conn, temp_dir) = create_test_db();
    let db = Arc::new(Mutex::new(conn));
    let image_cache = Arc::new(ImageCache::new(temp_dir.path()));

    let _router = create_router(db, image_cache);
    // If we got here without panicking, the router was created successfully
}

#[test]
fn test_app_state_clone() {
    let (conn, temp_dir) = create_test_db();
    let db = Arc::new(Mutex::new(conn));
    let image_cache = Arc::new(ImageCache::new(temp_dir.path()));

    let state = AppState {
        db: db.clone(),
        image_cache: image_cache.clone(),
    };

    // Test that AppState is Clone
    let _state2 = state.clone();
}

#[test]
fn test_buy_signals_default_limit() {
    let params = BuySignalsParams {
        limit: default_buy_signals_limit(),
    };
    assert_eq!(params.limit, 100);
}

#[test]
fn test_search_params_default_limit() {
    let params = SearchParams {
        q: "test".to_string(),
        limit: default_limit(),
    };

    assert_eq!(params.limit, 100);
}

#[test]
fn test_api_response_serialization() {
    let response: ApiResponse<Vec<i32>> = ApiResponse {
        success: true,
        data: Some(vec![1, 2, 3]),
        error: None,
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"data\":[1,2,3]"));
}

#[test]
fn test_api_response_error_serialization() {
    let response: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some("Test error".to_string()),
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"success\":false"));
    assert!(json.contains("\"error\":\"Test error\""));
    // data should be omitted when None
    assert!(!json.contains("\"data\""));
}
