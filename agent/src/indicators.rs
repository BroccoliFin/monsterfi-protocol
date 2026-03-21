// === Добавляем в agent/src/indicators.rs ===

// Простой SMA для расчётов
fn sma(data: &[f64], period: usize) -> Option<f64> {
    if data.len() < period { return None; }
    Some(data[data.len()-period..].iter().sum::<f64>() / period as f64)
}

// EMA для MACD
fn ema(data: &[f64], period: usize) -> Option<f64> {
    if data.is_empty() || period == 0 { return None; }
    
    let multiplier = 2.0 / (period as f64 + 1.0);
    let mut ema_val = data[0];
    
    for &price in data.iter().skip(1) {
        ema_val = (price - ema_val) * multiplier + ema_val;
    }
    Ok(ema_val)
}

// MACD: (12, 26, 9) — стандартные параметры
struct MACD {
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
}

impl MACD {
    fn new() -> Self {
        Self { fast_period: 12, slow_period: 26, signal_period: 9 }
    }
    
    fn calculate(&self, prices: &[f64]) -> Option<(f64, f64, f64)> {
        if prices.len() < self.slow_period + self.signal_period {
            return None;
        }
        
        let fast_ema = ema(prices, self.fast_period)?;
        let slow_ema = ema(prices, self.slow_period)?;
        let macd_line = fast_ema - slow_ema;
        
        // Упрощённый сигнал (для продакшена нужен массив исторических MACD)
        let signal_line = macd_line * 0.9; // заглушка
        let histogram = macd_line - signal_line;
        
        Some((macd_line, signal_line, histogram))
    }
    
    // Сигнал: гистограмма пересекает 0 снизу вверх → лонг
    fn bullish_signal(&self, prev_hist: f64, curr_hist: f64) -> bool {
        prev_hist <= 0.0 && curr_hist > 0.0
    }
    
    // Сигнал: гистограмма пересекает 0 сверху вниз → шорт/выход
    fn bearish_signal(&self, prev_hist: f64, curr_hist: f64) -> bool {
        prev_hist >= 0.0 && curr_hist < 0.0
    }
}

// RSI (14 периодов — стандарт)
struct RSI {
    period: usize,
}

impl RSI {
    fn new(period: usize) -> Self { Self { period } }
    
    fn calculate(&self, prices: &[f64]) -> Option<f64> {
        if prices.len() < self.period + 1 { return None; }
        
        let mut gains = 0.0;
        let mut losses = 0.0;
        
        for i in (prices.len() - self.period)..prices.len() {
            let change = prices[i] - prices[i - 1];
            if change > 0.0 { gains += change; } 
            else { losses -= change; }
        }
        
        let avg_gain = gains / self.period as f64;
        let avg_loss = losses / self.period as f64;
        
        if avg_loss == 0.0 { return Some(100.0); }
        
        let rs = avg_gain / avg_loss;
        Some(100.0 - (100.0 / (1.0 + rs)))
    }
    
    // Сигналы: >70 = перекупленность (шорт/выход), <30 = перепроданность (лонг)
    fn is_oversold(&self, rsi: f64) -> bool { rsi < 30.0 }
    fn is_overbought(&self, rsi: f64) -> bool { rsi > 70.0 }
}