pub fn calculate_ema(prices: &[f64], period: usize) -> Option<f64> {
    if prices.is_empty() {
        return None;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut val = prices[0];
    for &p in prices.iter().skip(1) {
        val = (p - val) * k + val;
    }
    Some(val)
}
