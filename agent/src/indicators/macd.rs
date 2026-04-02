use super::ma::calculate_ema;

pub fn calculate_macd_histogram(prices: &[f64]) -> Option<(f64, f64)> {
    if prices.len() < 35 {
        return None;
    }
    let fast = calculate_ema(prices, 12)?;
    let slow = calculate_ema(prices, 26)?;
    let macd = fast - slow;
    let signal = macd * 0.9;
    Some((macd - signal, macd))
}
