pub mod ma;
pub mod macd;
pub mod rsi;

pub use macd::calculate_macd_histogram;
pub use rsi::calculate_rsi;

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct IndicatorContext {
    pub rsi: f64,
    pub prev_rsi: Option<f64>,
    pub macd_histogram: f64,
    pub prev_macd_histogram: Option<f64>,
    pub macd_line: f64,
}
