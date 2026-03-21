// === agent/src/strategy.rs ===

use crate::indicators::{MACD, RSI};

pub struct MACD_RSIStrategy {
    macd: MACD,
    rsi: RSI,
    trailing_stop: TrailingStop,
    risk: RiskConfig,
    
    // Состояние
    in_position: bool,
    entry_price: f64,
    prev_histogram: Option<f64>,
    position_size: f64,
}

impl MACD_RSIStrategy {
    pub fn new(leverage: u8) -> Self {
        Self {
            macd: MACD::new(),
            rsi: RSI::new(14),
            trailing_stop: TrailingStop::new(1.5), // 1.5% трейлинг
            risk: RiskConfig {
                max_position_size: 1000.0,
                max_leverage: leverage,
                max_drawdown_pct: 10.0,
                stop_loss_pct: 2.0,
                take_profit_pct: 5.0,
            },
            in_position: false,
            entry_price: 0.0,
            prev_histogram: None,
            position_size: 0.01, // 0.01 BTC
        }
    }
    
    // Главный метод: принимает цены, возвращает действие
    pub fn evaluate(&mut self, prices: &[f64], current_price: f64) -> Option<Signal> {
        // 1. Считаем индикаторы
        let macd_result = self.macd.calculate(prices)?;
        let (_, _, histogram) = macd_result;
        let rsi_value = self.rsi.calculate(prices)?;
        
        let prev_hist = self.prev_histogram.unwrap_or(0.0);
        self.prev_histogram = Some(histogram);
        
        // 2. Проверяем выход по трейлинг-стопу
        if self.in_position {
            if let Some(stop_price) = self.trailing_stop.update(current_price, true) {
                return Some(Signal::Exit { reason: "trailing_stop", exit_price: stop_price });
            }
            
            // Жёсткий стоп-лосс
            let loss_pct = (current_price - self.entry_price) / self.entry_price * 100.0;
            if loss_pct <= -self.risk.stop_loss_pct {
                return Some(Signal::Exit { reason: "stop_loss", exit_price: current_price });
            }
            
            // Тейк-профит
            let profit_pct = (current_price - self.entry_price) / self.entry_price * 100.0;
            if profit_pct >= self.risk.take_profit_pct {
                return Some(Signal::Exit { reason: "take_profit", exit_price: current_price });
            }
        }
        
        // 3. Сигнал на вход: MACD бычий + RSI перепродан
        if !self.in_position 
            && self.macd.bullish_signal(prev_hist, histogram)
            && self.rsi.is_oversold(rsi_value) 
        {
            // Валидация риска
            if self.risk.validate_order(self.position_size, self.risk.max_leverage, current_price).is_ok() {
                self.in_position = true;
                self.entry_price = current_price;
                self.trailing_stop.update(current_price, true); // инициализируем трейлинг
                return Some(Signal::EnterLong { 
                    entry_price: current_price, 
                    size: self.position_size,
                    leverage: self.risk.max_leverage,
                });
            }
        }
        
        // 4. Сигнал на выход: MACD медвежий + RSI перекуплен
        if self.in_position 
            && self.macd.bearish_signal(prev_hist, histogram)
            && self.rsi.is_overbought(rsi_value)
        {
            self.in_position = false;
            return Some(Signal::Exit { reason: "macd_rsi_exit", exit_price: current_price });
        }
        
        None
    }
}

#[derive(Debug, Clone)]
pub enum Signal {
    EnterLong { entry_price: f64, size: f64, leverage: u8 },
    Exit { reason: &'static str, exit_price: f64 },
}