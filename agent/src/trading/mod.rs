// === src/trading/mod.rs ===

pub mod client;
pub mod exchange;

// Ре-экспорт для удобства импорта извне
pub use client::HyperliquidClient;
pub use exchange::{ExchangeClient, OrderResponse};
