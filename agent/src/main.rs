// MonsterFi Executor v0.8 — M1 эквивалент + LONG/SHORT support

use anyhow::Result;
use clap::Parser;
use dotenvy::dotenv;
use reqwest::Client;
use serde_json::json;
use std::{collections::VecDeque, time::{SystemTime, UNIX_EPOCH}};
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value = "macd_rsi")]
    strategy: String,
    #[arg(short, long, default_value = "20")]
    leverage: u8,
    #[arg(long, default_value = "true")]
    testnet: bool,
}

// === Направление позиции ===
#[derive(Clone, Copy, PartialEq, Debug)]
enum Position {
    None,
    Long,
    Short,
}

// === Индикаторы ===
fn ema(prices: &[f64], period: usize) -> Option<f64> {
    if prices.is_empty() { return None; }
    let k = 2.0 / (period as f64 + 1.0);
    let mut val = prices[0];
    for &p in prices.iter().skip(1) { val = (p - val) * k + val; }
    Some(val)
}

fn rsi(prices: &[f64], period: usize) -> Option<f64> {
    if prices.len() < period + 1 { return None; }
    let mut gain = 0.0; let mut loss = 0.0;
    for i in (prices.len() - period)..prices.len() {
        let diff = prices[i] - prices[i - 1];
        if diff > 0.0 { gain += diff; } else { loss -= diff; }
    }
    let avg_gain = gain / period as f64;
    let avg_loss = if loss == 0.0 { 0.0001 } else { loss / period as f64 };
    Some(100.0 - 100.0 / (1.0 + avg_gain / avg_loss))
}

fn macd_histogram(prices: &[f64]) -> Option<(f64, f64)> {
    if prices.len() < 35 { return None; }
    let fast = ema(prices, 12)?;
    let slow = ema(prices, 26)?;
    let macd = fast - slow;
    Some((macd - macd * 0.9, macd))
}

// === Цена (Hyperliquid → Binance) ===
async fn get_btc_price(client: &Client, api_base: &str) -> Result<f64> {
    // FIX: Убираем пробелы из URL
    let url = format!("{}/info", api_base.trim().trim_end_matches('/'));
    let body = json!({"type": "allMids"});

    if let Ok(resp) = client.post(&url).json(&body).timeout(Duration::from_secs(8)).send().await {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(p) = json.get("BTC").and_then(|v| v.as_str()) {
                        if let Ok(price) = p.parse::<f64>() {
                            info!("💰 Hyperliquid price: ${:.2}", price);
                            return Ok(price);
                        }
                    }
                }
            }
        }
    }

    // FIX: Убираем пробелы из Binance URL
    if let Ok(resp) = client.get("https://api.binance.com/api/v3/ticker/price?symbol=BTCUSDT")
        .timeout(Duration::from_secs(5)).send().await {
        if let Ok(text) = resp.text().await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(p) = json.get("price").and_then(|v| v.as_str()) {
                    if let Ok(price) = p.parse::<f64>() {
                        info!("💰 Binance price: ${:.2}", price);
                        return Ok(price);
                    }
                }
            }
        }
    }

    let fallback = 71_860.0 + (SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() % 500) as f64 / 10.0;
    warn!("⚠️ Fallback price: ${:.2}", fallback);
    Ok(fallback)
}

// === Entry: поддержка LONG и SHORT ===
async fn execute_entry(client: &Client, api_base: &str, size: f64, lev: u8, is_long: bool) {
    let order = json!({ 
        "type": "order", 
        "asset": "BTC", 
        "isBuy": is_long,  // ← Динамически: true=LONG, false=SHORT
        "reduceOnly": false, 
        "size": size.to_string(), 
        "leverage": lev, 
        "orderType": "Market" 
    });
    
    let direction = if is_long { "LONG" } else { "SHORT" };
    match client.post(format!("{}/exchange", api_base.trim().trim_end_matches('/'))).json(&order).send().await {
        Ok(r) => info!("📈 {} entry sent | Status: {}", direction, r.status()),
        Err(e) => error!("❌ {} entry failed: {}", direction, e),
    }
}

// === Exit: закрывает текущую позицию ===
async fn execute_exit(client: &Client, api_base: &str, size: f64, is_long: bool) {
    // Для закрытия: isBuy противоположно входу, reduceOnly=true
    let order = json!({ 
        "type": "order", 
        "asset": "BTC", 
        "isBuy": !is_long,  // ← Закрываем противоположной стороной
        "reduceOnly": true, 
        "size": size.to_string(), 
        "orderType": "Market" 
    });
    
    let direction = if is_long { "LONG" } else { "SHORT" };
    match client.post(format!("{}/exchange", api_base.trim().trim_end_matches('/'))).json(&order).send().await {
        Ok(r) => info!("📉 {} exit sent | Status: {}", direction, r.status()),
        Err(e) => error!("❌ {} exit failed: {}", direction, e),
    }
}

// === Трейлинг-стоп с поддержкой LONG/SHORT ===
#[derive(Debug, Clone)]
struct TrailingStop {
    highest_price: Option<f64>,  // для LONG: стоп подтягивается вверх
    lowest_price: Option<f64>,   // для SHORT: стоп подтягивается вниз
    trail_pct: f64,
}

impl TrailingStop {
    fn new(trail_pct: f64) -> Self {
        Self { highest_price: None, lowest_price: None, trail_pct }
    }

    // Возвращает цену стопа, если сработал
    fn update(&mut self, price: f64, position: Position) -> Option<f64> {
        match position {
            Position::Long => {
                // Для лонга: обновляем максимум, стоп = максимум - %
                if self.highest_price.is_none() || price > self.highest_price.unwrap() {
                    self.highest_price = Some(price);
                }
                let stop = self.highest_price.unwrap() * (1.0 - self.trail_pct / 100.0);
                if price <= stop { Some(stop) } else { None }
            }
            Position::Short => {
                // Для шорта: обновляем минимум, стоп = минимум + %
                if self.lowest_price.is_none() || price < self.lowest_price.unwrap() {
                    self.lowest_price = Some(price);
                }
                let stop = self.lowest_price.unwrap() * (1.0 + self.trail_pct / 100.0);
                if price >= stop { Some(stop) } else { None }  // Выход при росте цены
            }
            Position::None => None,
        }
    }
    
    // Сброс при закрытии позиции
    fn reset(&mut self) {
        self.highest_price = None;
        self.lowest_price = None;
    }
}

// === MAIN ===
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    dotenv().ok();

    let args = Args::parse();
    
    // FIX: Убираем пробелы из URL
    let api_base = if args.testnet { 
        "https://api.hyperliquid-testnet.xyz" 
    } else { 
        "https://api.hyperliquid.xyz" 
    };

    info!("🦖 MonsterFi Executor v0.8 started");
    info!("🎯 Strategy: {} | Leverage: {}x | LONG/SHORT | TF: M1 (6s × 500)", 
          args.strategy, args.leverage);

    let client = Client::new();
    
    // Состояние: направление позиции вместо простого bool
    let mut position = Position::None;
    let mut price_buffer: VecDeque<f64> = VecDeque::with_capacity(500);
    let mut trailing_stop = TrailingStop::new(1.5);  // 1.5% трейлинг
    let mut prev_histogram: Option<f64> = None;
    
    // Для логирования цены входа
    let mut entry_price: Option<f64> = None;

    let mut interval = interval(Duration::from_secs(6));

    loop {
        interval.tick().await;

        let current_price = get_btc_price(&client, api_base).await?;
        price_buffer.push_back(current_price);
        if price_buffer.len() > 500 { price_buffer.pop_front(); }

        let buffer_len = price_buffer.len();

        if buffer_len >= 35 {
            let prices: Vec<f64> = price_buffer.iter().copied().collect();
            
            if let Some((histogram, macd_line)) = macd_histogram(&prices) {
                let rsi_val = rsi(&prices, 14).unwrap_or(50.0);

                info!("📊 MACD: {:.4} | RSI: {:.1} | Pos: {:?}", histogram, rsi_val, position);

                // === ЛОГИКА ВХОДА (когда нет позиции) ===
                if position == Position::None {
                    let prev = prev_histogram.unwrap_or(0.0);
                    
                    // LONG условия: бычий кроссовер + перепроданность
                    let bullish_cross = prev <= 0.0 && histogram > 0.0;
                    let oversold = rsi_val < 45.0;
                    
                    // SHORT условия: медвежий кроссовер + перекупленность
                    let bearish_cross = prev >= 0.0 && histogram < 0.0;
                    let overbought = rsi_val > 55.0;

                    // Вход в LONG
                    if bullish_cross && oversold {
                        info!("🚀 SIGNAL LONG | MACD: {:.4} | RSI: {:.1} | Price: ${:.2}", 
                              macd_line, rsi_val, current_price);
                        execute_entry(&client, api_base, 0.01, args.leverage, true).await;
                        position = Position::Long;
                        entry_price = Some(current_price);
                        trailing_stop.reset();
                        trailing_stop.highest_price = Some(current_price);
                        
                    // Вход в SHORT
                    } else if bearish_cross && overbought {
                        info!("🚀 SIGNAL SHORT | MACD: {:.4} | RSI: {:.1} | Price: ${:.2}", 
                              macd_line, rsi_val, current_price);
                        execute_entry(&client, api_base, 0.01, args.leverage, false).await;
                        position = Position::Short;
                        entry_price = Some(current_price);
                        trailing_stop.reset();
                        trailing_stop.lowest_price = Some(current_price);
                        
                    } else {
                        // Дебаг: почему нет входа
                        if !bullish_cross && !bearish_cross {
                            info!("⏸️ No entry: MACD no crossover (hist: {:.4}, prev: {:.4})", histogram, prev);
                        } else if bullish_cross && !oversold {
                            info!("⏸️ No LONG: RSI not oversold ({:.1} > 45)", rsi_val);
                        } else if bearish_cross && !overbought {
                            info!("⏸️ No SHORT: RSI not overbought ({:.1} < 55)", rsi_val);
                        }
                    }
                }

                // === ЛОГИКА ВЫХОДА (когда есть позиция) ===
                if position != Position::None {
                    // Трейлинг-стоп
                    if let Some(stop_price) = trailing_stop.update(current_price, position) {
                        let pnl_pct = match position {
                            Position::Long => (current_price - entry_price.unwrap()) / entry_price.unwrap() * 100.0,
                            Position::Short => (entry_price.unwrap() - current_price) / entry_price.unwrap() * 100.0,
                            Position::None => 0.0,
                        };
                        info!("🛑 Trailing stop hit @ ${:.2} | PnL: {:.2}%", stop_price, pnl_pct);
                        execute_exit(&client, api_base, 0.01, position == Position::Long).await;
                        position = Position::None;
                        entry_price = None;
                        trailing_stop.reset();
                    }
                    
                    // Тейк-профит +5% (опционально)
                    if let Some(entry) = entry_price {
                        let pnl_pct = match position {
                            Position::Long => (current_price - entry) / entry * 100.0,
                            Position::Short => (entry - current_price) / entry * 100.0,
                            Position::None => 0.0,
                        };
                        if pnl_pct >= 5.0 {
                            info!("✅ Take-profit hit: +{:.1}%", pnl_pct);
                            execute_exit(&client, api_base, 0.01, position == Position::Long).await;
                            position = Position::None;
                            entry_price = None;
                            trailing_stop.reset();
                        }
                    }
                }

                prev_histogram = Some(histogram);
            }
        }

        // Heartbeat с PnL если в позиции
        let pnl_info = if position != Position::None && entry_price.is_some() {
            let entry = entry_price.unwrap();
            let pnl = match position {
                Position::Long => (current_price - entry) / entry * 100.0,
                Position::Short => (entry - current_price) / entry * 100.0,
                Position::None => 0.0,
            };
            format!(" | PnL: {:+.2}%", pnl)
        } else {
            String::new()
        };
        
        info!("💓 Heartbeat | Pos: {:?} | Price: ${:.2} | Buffer: {}/500{}", 
              position, current_price, buffer_len, pnl_info);
    }
}
