// MonsterFi Executor v0.9.1 — MACD+RSI Wave + LONG/SHORT + Backtest Support

use anyhow::Result;
use clap::Parser;
use dotenvy::dotenv;
use reqwest::Client;
use serde_json::json;
use std::collections::VecDeque;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

// === Backtest module ===
mod backtest;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value = "macd_rsi")]
    strategy: String,
    #[arg(short, long, default_value = "20")]
    leverage: u8,
    #[arg(long, default_value = "true")]
    testnet: bool,

    // === Backtest flags ===
    #[arg(long)]
    backtest: bool,
    #[arg(long, default_value = "BTCUSDT")]
    symbol: String,
    #[arg(long, default_value = "30m")]
    interval: String,
    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    to: Option<String>,
    #[arg(long)]
    sweep: bool,
    #[arg(long)]
    timeframe_sweep: bool,
}

// === Направление позиции ===
#[derive(Clone, Copy, PartialEq, Debug)]
enum Position {
    None,
    Long,
    Short,
}

// === Трейлинг-стоп ===
#[derive(Debug, Clone)]
struct TrailingStop {
    highest_price: Option<f64>,
    lowest_price: Option<f64>,
    trail_pct: f64,
}

impl TrailingStop {
    fn new(trail_pct: f64) -> Self {
        Self {
            highest_price: None,
            lowest_price: None,
            trail_pct,
        }
    }

    fn reset(&mut self) {
        self.highest_price = None;
        self.lowest_price = None;
    }

    fn update(&mut self, price: f64, position: Position) -> Option<f64> {
        match position {
            Position::Long => {
                if self.highest_price.is_none() || price > self.highest_price.unwrap() {
                    self.highest_price = Some(price);
                }
                let stop = self.highest_price.unwrap() * (1.0 - self.trail_pct / 100.0);
                if price <= stop {
                    Some(stop)
                } else {
                    None
                }
            }
            Position::Short => {
                if self.lowest_price.is_none() || price < self.lowest_price.unwrap() {
                    self.lowest_price = Some(price);
                }
                let stop = self.lowest_price.unwrap() * (1.0 + self.trail_pct / 100.0);
                if price >= stop {
                    Some(stop)
                } else {
                    None
                }
            }
            Position::None => None,
        }
    }
}

// === Индикаторы (идентично backtest.rs) ===
fn ema(prices: &[f64], period: usize) -> Option<f64> {
    if prices.is_empty() {
        return None;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut val = prices[0];
    for &p in prices.iter().skip(1) {
        val = (p - val) * k + val;
    }
    Some(val)
}

fn rsi(prices: &[f64], period: usize) -> Option<f64> {
    if prices.len() < period + 1 {
        return None;
    }
    let mut gain = 0.0;
    let mut loss = 0.0;
    for i in (prices.len() - period)..prices.len() {
        let diff = prices[i] - prices[i - 1];
        if diff > 0.0 {
            gain += diff;
        } else {
            loss -= diff;
        }
    }
    let avg_gain = gain / period as f64;
    let avg_loss = if loss == 0.0 {
        0.0001
    } else {
        loss / period as f64
    };
    Some(100.0 - 100.0 / (1.0 + avg_gain / avg_loss))
}

fn macd_histogram(prices: &[f64]) -> Option<(f64, f64)> {
    if prices.len() < 35 {
        return None;
    }
    let fast = ema(prices, 12)?;
    let slow = ema(prices, 26)?;
    let macd = fast - slow;
    let signal = macd * 0.9;
    Some((macd - signal, macd))
}

// === Цена (Hyperliquid → Binance fallback) ===
async fn get_crypto_price(client: &Client, symbol: &str, api_base: &str) -> Result<f64> {
    // Попытка получить цену с Hyperliquid
    let url = format!("{}/info", api_base.trim().trim_end_matches('/'));
    let body = json!({"type": "allMids"});

    if let Ok(resp) = client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(8))
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    let ticker = symbol.replace("USDT", "");
                    if let Some(p) = json.get(&ticker).and_then(|v| v.as_str()) {
                        if let Ok(price) = p.parse::<f64>() {
                            info!("💰 Hyperliquid price ({}): ${:.2}", ticker, price);
                            return Ok(price);
                        }
                    }
                }
            }
        }
    }

    // Фолбэк на Binance
    let binance_symbol = if symbol.contains("USDT") {
        symbol.to_string()
    } else {
        format!("{}USDT", symbol)
    };

    if let Ok(resp) = client
        .get(format!(
            "https://api.binance.com/api/v3/ticker/price?symbol={}",
            binance_symbol
        ))
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        if let Ok(text) = resp.text().await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(p) = json.get("price").and_then(|v| v.as_str()) {
                    if let Ok(price) = p.parse::<f64>() {
                        info!("💰 Binance price ({}): ${:.2}", binance_symbol, price);
                        return Ok(price);
                    }
                }
            }
        }
    }

    // Фолбэк цена (для тестов)
    let fallback = match symbol {
        "SOLUSDT" => 190.0,
        "ETHUSDT" => 3500.0,
        _ => 95000.0, // BTC
    };
    warn!("⚠️ Fallback price ({}): ${:.2}", symbol, fallback);
    Ok(fallback)
}

// === Entry / Exit ===
async fn execute_entry(
    client: &Client,
    api_base: &str,
    size: f64,
    lev: u8,
    is_long: bool,
    symbol: &str,
) {
    let asset = symbol.replace("USDT", "");
    let order = json!({
        "type": "order",
        "asset": asset,
        "isBuy": is_long,
        "reduceOnly": false,
        "size": size.to_string(),
        "leverage": lev,
        "orderType": "Market"
    });

    let direction = if is_long { "LONG" } else { "SHORT" };
    match client
        .post(format!(
            "{}/exchange",
            api_base.trim().trim_end_matches('/')
        ))
        .json(&order)
        .send()
        .await
    {
        Ok(r) => info!(
            "📈 {} entry sent ({}) | Status: {}",
            direction,
            symbol,
            r.status()
        ),
        Err(e) => error!("❌ {} entry failed ({}): {}", direction, symbol, e),
    }
}

async fn execute_exit(client: &Client, api_base: &str, size: f64, is_long: bool, symbol: &str) {
    let asset = symbol.replace("USDT", "");
    let order = json!({
        "type": "order",
        "asset": asset,
        "isBuy": !is_long,
        "reduceOnly": true,
        "size": size.to_string(),
        "orderType": "Market"
    });

    let direction = if is_long { "LONG" } else { "SHORT" };
    match client
        .post(format!(
            "{}/exchange",
            api_base.trim().trim_end_matches('/')
        ))
        .json(&order)
        .send()
        .await
    {
        Ok(r) => info!(
            "📉 {} exit sent ({}) | Status: {}",
            direction,
            symbol,
            r.status()
        ),
        Err(e) => error!("❌ {} exit failed ({}): {}", direction, symbol, e),
    }
}

// === Backtest mode ===
async fn run_backtest_mode(args: &Args) -> Result<()> {
    use backtest::{
        fetch_binance_klines, print_results_table, print_single_result, print_timeframe_comparison,
        run_backtest, run_param_sweep, run_timeframe_sweep, StrategyParams,
    };
    use chrono::{NaiveDate, Utc};

    info!("🧪 Starting backtest mode");

    let parse_date = |date_str: &str| -> Result<i64> {
        if let Ok(ts) = date_str.parse::<i64>() {
            return Ok(ts * 1000);
        }
        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .map_err(|e| anyhow::anyhow!("Invalid date '{}': {}", date_str, e))?;
        let dt = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow::anyhow!("Invalid time"))?
            .and_utc();
        Ok(dt.timestamp_millis())
    };

    let end_time = if let Some(to) = &args.to {
        parse_date(to)?
    } else {
        Utc::now().timestamp_millis()
    };

    let start_time = if let Some(from) = &args.from {
        parse_date(from)?
    } else {
        (Utc::now() - chrono::Duration::days(7)).timestamp_millis()
    };

    if args.timeframe_sweep {
        info!("🔄 Запуск параметрического поиска по таймфреймам...");
        let results = run_timeframe_sweep(
            &args.symbol,
            start_time,
            end_time,
            &StrategyParams::default(),
        )
        .await?;
        print_timeframe_comparison(&results);
        return Ok(());
    }

    info!(
        "📅 Fetching {} [{}] from {} to {}",
        args.symbol,
        args.interval,
        chrono::DateTime::from_timestamp_millis(start_time).unwrap(),
        chrono::DateTime::from_timestamp_millis(end_time).unwrap()
    );

    let candles =
        fetch_binance_klines(&args.symbol, &args.interval, start_time, end_time, 200000).await?;

    if args.sweep {
        info!("🔍 Running parameter sweep...");
        let sweep = backtest::ParamSweep::default();
        let results = run_param_sweep(&candles, &sweep).await?;
        print_results_table(&results, 10);

        if let Some(best) = results.first() {
            info!(
                "🏆 Best params: RSI[{:.0}/{:.0}] Trail[{:.1}%] TP[{:.0}%] → Return: {:+.2}%",
                best.0.rsi_oversold,
                best.0.rsi_overbought,
                best.0.trail_pct,
                best.0.take_profit_pct,
                best.1.total_return_pct
            );
        }
    } else {
        let params = StrategyParams::default();
        let result = run_backtest(&candles, &params).await?;
        print_single_result(&result);
    }

    Ok(())
}

// === LIVE trading mode ===
async fn run_live_mode(args: &Args) -> Result<()> {
    let api_base = if args.testnet {
        "https://api.hyperliquid-testnet.xyz"
    } else {
        "https://api.hyperliquid.xyz"
    };

    // ✅ ПАРАМЕТРЫ идентичны StrategyParams::default() из backtest.rs
    let rsi_oversold = 30.0;
    let rsi_overbought = 70.0;
    let trail_pct = 3.5;
    let take_profit_pct = 6.0;
    let leverage = args.leverage; // из аргументов, дефолт 20
    let max_loss_pct = 9.0;

    info!("🦖 MonsterFi Executor v0.9.1 started");
    info!(
        "🎯 Strategy: {} | Leverage: {}x | Symbol: {} | RSI Wave: {:.0}/{:.0}",
        args.strategy, leverage, args.symbol, rsi_oversold, rsi_overbought
    );

    let client = Client::new();

    let mut position = Position::None;
    let mut price_buffer: VecDeque<f64> = VecDeque::with_capacity(500);
    let mut trailing_stop = TrailingStop::new(trail_pct);
    let mut prev_histogram: Option<f64> = None;
    let mut entry_price: Option<f64> = None;

    // ✅ RSI WAVE 2-STATE: отслеживаем предыдущий RSI
    let mut prev_rsi: Option<f64> = None;

    let mut interval = interval(Duration::from_secs(6));

    loop {
        interval.tick().await;

        let current_price = get_crypto_price(&client, &args.symbol, api_base).await?;
        price_buffer.push_back(current_price);
        if price_buffer.len() > 500 {
            price_buffer.pop_front();
        }

        let buffer_len = price_buffer.len();

        if buffer_len >= 35 {
            let prices: Vec<f64> = price_buffer.iter().copied().collect();

            if let Some((histogram, _macd_line)) = macd_histogram(&prices) {
                let rsi_val = rsi(&prices, 14).unwrap_or(50.0);

                info!(
                    "📊 MACD: {:.4} | RSI: {:.1} (prev: {:.1}) | Pos: {:?}",
                    histogram,
                    rsi_val,
                    prev_rsi.unwrap_or(50.0),
                    position
                );

                // ✅ RSI WAVE 2-STATE: prev + current (идентично backtest.rs)
                let rsi_exit_oversold =
                    prev_rsi.map_or(false, |p| p < rsi_oversold) && rsi_val > rsi_oversold;

                let rsi_exit_overbought =
                    prev_rsi.map_or(false, |p| p > rsi_overbought) && rsi_val < rsi_overbought;

                // === ВХОД ===
                if position == Position::None {
                    let prev = prev_histogram.unwrap_or(0.0);
                    let bullish_cross = prev <= 0.0 && histogram > 0.0;
                    let bearish_cross = prev >= 0.0 && histogram < 0.0;

                    info!("🔍 Entry | BullishCross: {} | RSI_exit_oversold: {} | BearishCross: {} | RSI_exit_overbought: {}",
              bullish_cross, rsi_exit_oversold, bearish_cross, rsi_exit_overbought);

                    // ✅ LONG: MACD кросс + выход RSI из перепроданности
                    if bullish_cross && rsi_exit_oversold {
                        info!(
                            "🚀 SIGNAL LONG (RSI WAVE) | MACD: {:.4} | RSI: {:.1} (prev: {:.1}) | Price: ${:.2}",
                            _macd_line, rsi_val, prev_rsi.unwrap_or(0.0), current_price
                        );
                        execute_entry(&client, api_base, 0.01, leverage, true, &args.symbol).await;
                        position = Position::Long;
                        entry_price = Some(current_price);
                        trailing_stop.reset();
                        trailing_stop.highest_price = Some(current_price);
                    }
                    // ✅ SHORT: MACD кросс + выход RSI из перекупленности
                    else if bearish_cross && rsi_exit_overbought {
                        info!(
                            "🚀 SIGNAL SHORT (RSI WAVE) | MACD: {:.4} | RSI: {:.1} (prev: {:.1}) | Price: ${:.2}",
                            _macd_line, rsi_val, prev_rsi.unwrap_or(0.0), current_price
                        );
                        execute_entry(&client, api_base, 0.01, leverage, false, &args.symbol).await;
                        position = Position::Short;
                        entry_price = Some(current_price);
                        trailing_stop.reset();
                        trailing_stop.lowest_price = Some(current_price);
                    }
                }

                // === ВЫХОД ===
                if position != Position::None {
                    let mut should_exit = false;
                    let is_long = position == Position::Long;

                    // ✅ HARD STOP-LOSS (идентично backtest.rs)
                    let price_stop_loss = if is_long {
                        entry_price.unwrap() * (1.0 - max_loss_pct / 100.0 / leverage as f64)
                    } else {
                        entry_price.unwrap() * (1.0 + max_loss_pct / 100.0 / leverage as f64)
                    };

                    if (is_long && current_price <= price_stop_loss)
                        || (!is_long && current_price >= price_stop_loss)
                    {
                        should_exit = true;
                        info!(
                            "🛑 HARD STOP-LOSS @ ${:.2} (entry: ${:.2}, loss: -{:.2}%)",
                            current_price,
                            entry_price.unwrap(),
                            max_loss_pct
                        );
                    }

                    // ✅ TRAILING STOP
                    if !should_exit {
                        if let Some(stop_price) = trailing_stop.update(current_price, position) {
                            should_exit = true;
                            info!("🛑 Trailing stop hit @ ${:.2}", stop_price);
                        }
                    }

                    // ✅ TAKE PROFIT
                    if !should_exit {
                        let raw_pnl = if is_long {
                            (current_price - entry_price.unwrap()) / entry_price.unwrap() * 100.0
                        } else {
                            (entry_price.unwrap() - current_price) / entry_price.unwrap() * 100.0
                        };
                        if raw_pnl >= take_profit_pct {
                            should_exit = true;
                            info!("✅ Take-profit hit: +{:.1}%", raw_pnl);
                        }
                    }

                    // === Исполнение выхода ===
                    if should_exit {
                        execute_exit(&client, api_base, 0.01, is_long, &args.symbol).await;

                        let pnl_pct = if is_long {
                            (current_price - entry_price.unwrap()) / entry_price.unwrap()
                                * 100.0
                                * leverage as f64
                                - 0.08
                        } else {
                            (entry_price.unwrap() - current_price) / entry_price.unwrap()
                                * 100.0
                                * leverage as f64
                                - 0.08
                        };

                        info!(
                            "📊 TRADE: {} | Entry: ${:.2} | Exit: ${:.2} | PnL: {:+.2}%",
                            if is_long { "LONG" } else { "SHORT" },
                            entry_price.unwrap(),
                            current_price,
                            pnl_pct
                        );

                        position = Position::None;
                        entry_price = None;
                        trailing_stop.reset();
                    }
                }

                prev_histogram = Some(histogram);
                // ✅ ФИКС: Обновляем prev_rsi ВНУТРИ блока где определён rsi_val
                prev_rsi = Some(rsi_val);
            }
        }

        // Логирование статуса
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

        info!(
            "💓 Heartbeat | Pos: {:?} | Price: ${:.2} | Buffer: {}/500{}",
            position, current_price, buffer_len, pnl_info
        );
    }
}

// === MAIN ===
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    dotenv().ok();

    let args = Args::parse();

    if args.backtest {
        run_backtest_mode(&args).await?;
        return Ok(());
    }

    run_live_mode(&args).await?;

    Ok(())
}
