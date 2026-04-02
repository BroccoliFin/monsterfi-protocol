use crate::indicators::IndicatorContext;

pub mod macd_rsi;
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
    fn get_rsi_oversold(&self) -> f64;
    fn get_rsi_overbought(&self) -> f64;
    fn get_trail_pct(&self) -> f64;
    fn get_take_profit_pct(&self) -> f64;
    fn get_max_loss_pct(&self) -> f64;
}
