pub mod ao;
pub mod ma;
pub mod macd;
pub mod rsi;

pub use ao::calculate_ao;
//pub use ma::calculate_ema;
pub use macd::calculate_macd_histogram;
pub use rsi::calculate_rsi;

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct IndicatorContext {
    // RSI
    pub rsi: f64,
    pub prev_rsi: Option<f64>,

    // MACD
    pub macd_histogram: f64,
    pub prev_macd_histogram: Option<f64>,
    pub macd_line: f64,

    // AO (4 значения для 3-барного паттерна)
    pub ao: Option<f64>,
    pub prev_ao: Option<f64>,
    pub prev_prev_ao: Option<f64>,
    pub prev_prev_prev_ao: Option<f64>,

    // Свечи OHLC (3 бара)
    pub current_candle: Option<(f64, f64, f64, f64)>,
    pub prev_candle: Option<(f64, f64, f64, f64)>,
    pub prev_prev_candle: Option<(f64, f64, f64, f64)>,
}
