//! Tests for indicators.

use super::*;

#[test]
fn test_ema_basic() {
    let prices = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
    let ema = calculate_ema(&prices, 3);

    // First 2 values should be None
    assert!(ema[0].is_none());
    assert!(ema[1].is_none());
    // Third value should be SMA of first 3
    assert!((ema[2].unwrap() - 2.0).abs() < 0.001);
    // All subsequent should be Some
    assert!(ema[9].is_some());
}

#[test]
fn test_sma_basic() {
    let prices = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let sma = calculate_sma(&prices, 3);

    assert!(sma[0].is_none());
    assert!(sma[1].is_none());
    assert!((sma[2].unwrap() - 2.0).abs() < 0.001);
    assert!((sma[3].unwrap() - 3.0).abs() < 0.001);
    assert!((sma[4].unwrap() - 4.0).abs() < 0.001);
}

#[test]
fn test_rsi_basic() {
    // Steadily increasing prices should have RSI > 50
    let prices: Vec<f64> = (1..=20).map(|x| x as f64).collect();
    let rsi = calculate_rsi(&prices, 14);

    // First 14 values should be None
    for item in rsi.iter().take(14) {
        assert!(item.is_none());
    }
    // RSI for steadily increasing prices should be 100
    assert!((rsi[14].unwrap() - 100.0).abs() < 0.001);
}

#[test]
fn test_macd_returns_correct_lengths() {
    let prices: Vec<f64> = (1..=50).map(|x| x as f64).collect();
    let (macd, signal, histogram) = calculate_macd(&prices);

    assert_eq!(macd.len(), 50);
    assert_eq!(signal.len(), 50);
    assert_eq!(histogram.len(), 50);

    // First 25 values should be None (need 26 for EMA26)
    assert!(macd[24].is_none());
    // Value at index 25 should exist
    assert!(macd[25].is_some());
}

#[test]
fn test_bollinger_bands_basic() {
    let prices: Vec<f64> = (1..=25).map(|x| x as f64).collect();
    let (upper, middle, lower) = calculate_bollinger_bands(&prices, 20, 2.0);

    assert_eq!(upper.len(), 25);
    // First 19 should be None
    assert!(upper[18].is_none());
    // Value at index 19 should exist
    assert!(upper[19].is_some());
    assert!(middle[19].is_some());
    assert!(lower[19].is_some());
    // Upper > Middle > Lower
    assert!(upper[19].unwrap() > middle[19].unwrap());
    assert!(middle[19].unwrap() > lower[19].unwrap());
}

#[test]
fn test_empty_prices() {
    let empty: Vec<f64> = vec![];
    assert!(calculate_ema(&empty, 7).is_empty());
    assert!(calculate_sma(&empty, 20).is_empty());
    assert!(calculate_rsi(&empty, 14).is_empty());

    let (m, s, h) = calculate_macd(&empty);
    assert!(m.is_empty());
    assert!(s.is_empty());
    assert!(h.is_empty());

    let (u, mid, l) = calculate_bollinger_bands(&empty, 20, 2.0);
    assert!(u.is_empty());
    assert!(mid.is_empty());
    assert!(l.is_empty());
}

#[test]
fn test_calculate_all_indicators() {
    let prices: Vec<f64> = (1..=50).map(|x| x as f64 * 0.1).collect();
    let indicators = calculate_all_indicators(&prices);

    assert_eq!(indicators.ema_7.len(), 50);
    assert_eq!(indicators.ema_30.len(), 50);
    assert_eq!(indicators.sma_20.len(), 50);
    assert_eq!(indicators.rsi.len(), 50);
    assert_eq!(indicators.macd.len(), 50);
    assert_eq!(indicators.macd_signal.len(), 50);
    assert_eq!(indicators.macd_histogram.len(), 50);
    assert_eq!(indicators.bb_upper.len(), 50);
    assert_eq!(indicators.bb_middle.len(), 50);
    assert_eq!(indicators.bb_lower.len(), 50);
}
