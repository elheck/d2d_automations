//! Technical analysis indicators for price data
//!
//! Provides common stock trading indicators adapted for MTG card prices.

use serde::Serialize;

/// Type alias for MACD result (macd_line, signal_line, histogram)
pub type MacdResult = (Vec<Option<f64>>, Vec<Option<f64>>, Vec<Option<f64>>);

/// Type alias for Bollinger Bands result (upper_band, middle_band, lower_band)
pub type BollingerBandsResult = (Vec<Option<f64>>, Vec<Option<f64>>, Vec<Option<f64>>);

/// All technical indicators for a price series
#[derive(Debug, Serialize)]
pub struct TechnicalIndicators {
    pub ema_7: Vec<Option<f64>>,
    pub ema_30: Vec<Option<f64>>,
    pub sma_20: Vec<Option<f64>>,
    pub rsi: Vec<Option<f64>>,
    pub macd: Vec<Option<f64>>,
    pub macd_signal: Vec<Option<f64>>,
    pub macd_histogram: Vec<Option<f64>>,
    pub bb_upper: Vec<Option<f64>>,
    pub bb_middle: Vec<Option<f64>>,
    pub bb_lower: Vec<Option<f64>>,
}

/// Calculate all technical indicators for a price series
pub fn calculate_all_indicators(prices: &[f64]) -> TechnicalIndicators {
    let ema_7 = calculate_ema(prices, 7);
    let ema_30 = calculate_ema(prices, 30);
    let sma_20 = calculate_sma(prices, 20);
    let rsi = calculate_rsi(prices, 14);
    let (macd, macd_signal, macd_histogram) = calculate_macd(prices);
    let (bb_upper, bb_middle, bb_lower) = calculate_bollinger_bands(prices, 20, 2.0);

    TechnicalIndicators {
        ema_7,
        ema_30,
        sma_20,
        rsi,
        macd,
        macd_signal,
        macd_histogram,
        bb_upper,
        bb_middle,
        bb_lower,
    }
}

/// Calculate Exponential Moving Average (EMA)
///
/// EMA gives more weight to recent prices.
/// Multiplier = 2 / (period + 1)
pub fn calculate_ema(prices: &[f64], period: usize) -> Vec<Option<f64>> {
    if prices.is_empty() || period == 0 || period > prices.len() {
        return vec![None; prices.len()];
    }

    let mut result = vec![None; prices.len()];
    let multiplier = 2.0 / (period as f64 + 1.0);

    // First EMA value is the SMA of the first `period` values
    let first_sma: f64 = prices[..period].iter().sum::<f64>() / period as f64;
    result[period - 1] = Some(first_sma);

    // Calculate EMA for remaining values
    let mut prev_ema = first_sma;
    for i in period..prices.len() {
        let ema = (prices[i] - prev_ema) * multiplier + prev_ema;
        result[i] = Some(ema);
        prev_ema = ema;
    }

    result
}

/// Calculate Simple Moving Average (SMA)
pub fn calculate_sma(prices: &[f64], period: usize) -> Vec<Option<f64>> {
    if prices.is_empty() || period == 0 || period > prices.len() {
        return vec![None; prices.len()];
    }

    let mut result = vec![None; prices.len()];

    for i in (period - 1)..prices.len() {
        let start = i.saturating_sub(period - 1);
        let sum: f64 = prices[start..=i].iter().sum();
        result[i] = Some(sum / period as f64);
    }

    result
}

/// Calculate MACD (Moving Average Convergence Divergence)
///
/// Returns (macd_line, signal_line, histogram)
/// - MACD Line: 12 EMA - 26 EMA
/// - Signal Line: 9 EMA of MACD
/// - Histogram: MACD - Signal
pub fn calculate_macd(prices: &[f64]) -> MacdResult {
    let len = prices.len();

    if len < 26 {
        return (vec![None; len], vec![None; len], vec![None; len]);
    }

    let ema_12 = calculate_ema(prices, 12);
    let ema_26 = calculate_ema(prices, 26);

    // MACD line = EMA12 - EMA26
    let mut macd_line = vec![None; len];
    let mut macd_values: Vec<f64> = Vec::new();

    for i in 0..len {
        if let (Some(e12), Some(e26)) = (ema_12[i], ema_26[i]) {
            let macd_val = e12 - e26;
            macd_line[i] = Some(macd_val);
            macd_values.push(macd_val);
        }
    }

    // Signal line = 9 EMA of MACD values
    let signal_ema = calculate_ema(&macd_values, 9);

    // Map signal EMA back to full-length array
    let mut signal_line = vec![None; len];
    let mut histogram = vec![None; len];
    let macd_start = len - macd_values.len();

    for (j, sig) in signal_ema.iter().enumerate() {
        let i = macd_start + j;
        signal_line[i] = *sig;
        if let (Some(m), Some(s)) = (macd_line[i], sig) {
            histogram[i] = Some(m - s);
        }
    }

    (macd_line, signal_line, histogram)
}

/// Calculate RSI (Relative Strength Index)
///
/// RSI = 100 - (100 / (1 + RS))
/// RS = Average Gain / Average Loss over `period` periods
pub fn calculate_rsi(prices: &[f64], period: usize) -> Vec<Option<f64>> {
    let len = prices.len();
    if len <= period || period == 0 {
        return vec![None; len];
    }

    let mut result = vec![None; len];

    // Calculate initial average gain/loss
    let mut avg_gain = 0.0;
    let mut avg_loss = 0.0;

    for i in 1..=period {
        let change = prices[i] - prices[i - 1];
        if change > 0.0 {
            avg_gain += change;
        } else {
            avg_loss += change.abs();
        }
    }

    avg_gain /= period as f64;
    avg_loss /= period as f64;

    // First RSI value
    if avg_loss == 0.0 {
        result[period] = Some(100.0);
    } else {
        let rs = avg_gain / avg_loss;
        result[period] = Some(100.0 - (100.0 / (1.0 + rs)));
    }

    // Calculate remaining RSI values using smoothed averages
    for i in (period + 1)..len {
        let change = prices[i] - prices[i - 1];
        let (gain, loss) = if change > 0.0 {
            (change, 0.0)
        } else {
            (0.0, change.abs())
        };

        avg_gain = (avg_gain * (period as f64 - 1.0) + gain) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + loss) / period as f64;

        if avg_loss == 0.0 {
            result[i] = Some(100.0);
        } else {
            let rs = avg_gain / avg_loss;
            result[i] = Some(100.0 - (100.0 / (1.0 + rs)));
        }
    }

    result
}

/// Calculate Bollinger Bands
///
/// Returns (upper_band, middle_band, lower_band)
/// - Middle: 20-period SMA
/// - Upper: Middle + 2 * standard deviation
/// - Lower: Middle - 2 * standard deviation
pub fn calculate_bollinger_bands(
    prices: &[f64],
    period: usize,
    std_dev_multiplier: f64,
) -> BollingerBandsResult {
    let len = prices.len();
    if len < period || period == 0 {
        return (vec![None; len], vec![None; len], vec![None; len]);
    }

    let mut upper = vec![None; len];
    let mut middle = vec![None; len];
    let mut lower = vec![None; len];

    for i in (period - 1)..len {
        let start = i.saturating_sub(period - 1);
        let window = &prices[start..=i];
        let mean: f64 = window.iter().sum::<f64>() / period as f64;

        let variance: f64 = window.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / period as f64;
        let std_dev = variance.sqrt();

        middle[i] = Some(mean);
        upper[i] = Some(mean + std_dev_multiplier * std_dev);
        lower[i] = Some(mean - std_dev_multiplier * std_dev);
    }

    (upper, middle, lower)
}

#[cfg(test)]
mod tests {
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
}
