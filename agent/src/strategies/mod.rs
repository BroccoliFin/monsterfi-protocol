use crate::indicators::IndicatorContext;

pub mod ao_candle;
pub mod macd_rsi;

pub use ao_candle::AoCandleStrategy;
pub use macd_rsi::MacdRsiStrategy;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Signal {
    Long,
    Short,
    Hold,
}

pub trait Strategy {
    fn name(&self) -> &str;
    fn generate_signal(&self, ctx: &IndicatorContext) -> Signal;
    #[allow(dead_code)]
    fn get_rsi_oversold(&self) -> f64;
    #[allow(dead_code)]
    fn get_rsi_overbought(&self) -> f64;
    fn get_trail_pct(&self) -> f64;
    fn get_take_profit_pct(&self) -> f64;
    fn get_max_loss_pct(&self) -> f64;
}
