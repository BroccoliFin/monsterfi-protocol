use super::{Signal, Strategy};
use crate::indicators::IndicatorContext;

pub struct MacdRsiStrategy {
    pub rsi_oversold: f64,
    pub rsi_overbought: f64,
    pub trail_pct: f64,
    pub take_profit_pct: f64,
    pub max_loss_pct: f64,
}

impl MacdRsiStrategy {
    pub fn new(
        rsi_oversold: f64,
        rsi_overbought: f64,
        trail_pct: f64,
        take_profit_pct: f64,
        max_loss_pct: f64,
    ) -> Self {
        Self {
            rsi_oversold,
            rsi_overbought,
            trail_pct,
            take_profit_pct,
            max_loss_pct,
        }
    }
}

impl Strategy for MacdRsiStrategy {
    fn name(&self) -> &str {
        "MACD+RSI Wave"
    }

    fn generate_signal(&self, ctx: &IndicatorContext) -> Signal {
        let rsi_exit_oversold =
            ctx.prev_rsi.is_some_and(|p| p < self.rsi_oversold) && ctx.rsi > self.rsi_oversold;

        let rsi_exit_overbought =
            ctx.prev_rsi.is_some_and(|p| p > self.rsi_overbought) && ctx.rsi < self.rsi_overbought;

        let bullish_cross =
            ctx.prev_macd_histogram.is_some_and(|p| p <= 0.0) && ctx.macd_histogram > 0.0;

        let bearish_cross =
            ctx.prev_macd_histogram.is_some_and(|p| p >= 0.0) && ctx.macd_histogram < 0.0;

        if bullish_cross && rsi_exit_oversold {
            Signal::Long
        } else if bearish_cross && rsi_exit_overbought {
            Signal::Short
        } else {
            Signal::Hold
        }
    }

    fn get_rsi_oversold(&self) -> f64 {
        self.rsi_oversold
    }
    fn get_rsi_overbought(&self) -> f64 {
        self.rsi_overbought
    }
    fn get_trail_pct(&self) -> f64 {
        self.trail_pct
    }
    fn get_take_profit_pct(&self) -> f64 {
        self.take_profit_pct
    }
    fn get_max_loss_pct(&self) -> f64 {
        self.max_loss_pct
    }
}
