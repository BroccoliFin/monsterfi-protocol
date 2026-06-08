#[allow(dead_code)]
pub fn calculate_ao(highs: &[f64], lows: &[f64], fast: usize, slow: usize) -> Option<f64> {
    if highs.len() < slow || lows.len() < slow {
        return None;
    }

    let medians: Vec<f64> = highs
        .iter()
        .zip(lows.iter())
        .map(|(h, l)| (h + l) / 2.0)
        .collect();

    let sma_fast = medians.iter().rev().take(fast).sum::<f64>() / fast as f64;
    let sma_slow = medians.iter().rev().take(slow).sum::<f64>() / slow as f64;

    Some(sma_fast - sma_slow)
}
