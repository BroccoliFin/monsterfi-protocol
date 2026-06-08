// === MonsterFi Executor Library ===
// Модульная архитектура для токенизированных торговых стратегий

// === Торговые модули (HyperLiquid интеграция) ===
pub mod trading;

// === Торговые стратегии ===
pub mod strategies;

// === Технические индикаторы ===
pub mod indicators;

// === Бэктест движок ===
pub mod backtest;

// === Ре-экспорт ключевых типов для удобства импорта ===
pub use indicators::IndicatorContext;
pub use strategies::{AoCandleStrategy, MacdRsiStrategy, Signal, Strategy};
pub use trading::{ExchangeClient, HyperliquidClient, OrderResponse};

// === Конфигурация по умолчанию ===
#[derive(Debug, Clone)]
pub struct Config {
    pub testnet: bool,
    pub leverage: u8,
    pub default_size: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            testnet: true,
            leverage: 20,
            default_size: 0.01,
        }
    }
}
