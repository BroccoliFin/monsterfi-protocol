use super::{Signal, Strategy};
use crate::indicators::IndicatorContext;

pub struct AoCandleStrategy {
    #[allow(dead_code)]
    pub ao_fast: usize,
    #[allow(dead_code)]
    pub ao_slow: usize,
    pub trail_pct: f64,
    pub take_profit_pct: f64,
    pub max_loss_pct: f64,
}

impl AoCandleStrategy {
    pub fn new(
        ao_fast: usize,
        ao_slow: usize,
        trail_pct: f64,
        take_profit_pct: f64,
        max_loss_pct: f64,
    ) -> Self {
        Self {
            ao_fast,
            ao_slow,
            trail_pct,
            take_profit_pct,
            max_loss_pct,
        }
    }
}

impl Strategy for AoCandleStrategy {
    fn name(&self) -> &str {
        "AO Saucer + Candle Breakout"
    }

    fn generate_signal(&self, ctx: &IndicatorContext) -> Signal {
        // 1. Проверяем наличие 3 значений AO (для 3-барного паттерна)
        let (ao_0, ao_m1, ao_m2, ao_m3) =
            match (ctx.ao, ctx.prev_ao, ctx.prev_prev_ao, ctx.prev_prev_prev_ao) {
                (Some(a0), Some(a1), Some(a2), Some(a3)) => (a0, a1, a2, a3),
                _ => return Signal::Hold,
            };

        // 2. Проверяем наличие свечей (текущая и предыдущая)
        let (candle_0, candle_m1) = match (ctx.current_candle, ctx.prev_candle) {
            (Some(c0), Some(c1)) => (c0, c1),
            _ => return Signal::Hold,
        };

        let (_open_0, _high_0, _low_0, _close_0) = candle_0;
        let (_open_m1, _high_m1, _low_m1, _close_m1) = candle_m1;

        // 3. Определяем цвета AO (зелёный = растёт, красный = падает)
        let ao_m2_green = ao_m2 > ao_m3; // бар -2
        let ao_m1_green = ao_m1 > ao_m2; // бар -1
        let ao_0_green = ao_0 > ao_m1; // бар 0

        // 4. BUY сигнал (AO Saucer ниже 0 + пробой high)
        let buy_signal =
            !ao_m2_green && ao_m1_green && ao_0_green && ao_m2 < 0.0 && ao_m1 < 0.0 && ao_0 < 0.0;
        // close_0 > high_m1  // ← отключено для тиков

        let sell_signal =
            ao_m2_green && !ao_m1_green && !ao_0_green && ao_m2 > 0.0 && ao_m1 > 0.0 && ao_0 > 0.0;
        // close_0 < low_m1  // ← отключено для тиков

        // 6. Финальный сигнал
        if buy_signal {
            Signal::Long
        } else if sell_signal {
            Signal::Short
        } else {
            Signal::Hold
        }
    }

    // Стандартные геттеры
    fn get_rsi_oversold(&self) -> f64 {
        30.0
    }
    fn get_rsi_overbought(&self) -> f64 {
        70.0
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
