pub fn calculate_rsi(prices: &[f64], period: usize) -> Option<f64> {
    if prices.len() < period + 1 {
        return None;
    }
    let mut gain = 0.0;
    let mut loss = 0.0;
    for i in (prices.len() - period)..prices.len() {
        let diff = prices[i] - prices[i - 1];
        if diff > 0.0 {
            gain += diff;
        } else {
            loss -= diff;
        }
    }
    let avg_gain = gain / period as f64;
    let avg_loss = if loss == 0.0 {
        0.0001
    } else {
        loss / period as f64
    };
    Some(100.0 - 100.0 / (1.0 + avg_gain / avg_loss))
}
