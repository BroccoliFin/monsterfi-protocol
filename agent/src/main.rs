// src/main.rs
// MonsterFi Executor - Main Entry Point & Trading Loop

use anyhow::Result;
use clap::Parser;
use dotenvy::dotenv;
use std::collections::VecDeque;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

// === Trading clients ===
use monsterfi_executor::trading::{ExchangeClient, HyperliquidClient};

mod backtest;
mod indicators;
mod strategies;

use backtest::{
    fetch_binance_klines, print_results_table, print_single_result, print_timeframe_comparison,
    run_backtest, run_param_sweep, run_timeframe_sweep,
};
use indicators::{calculate_ao, calculate_macd_histogram, calculate_rsi, IndicatorContext};
use strategies::{AoCandleStrategy, MacdRsiStrategy, Signal, Strategy};

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value = "macd_rsi")]
    strategy: String,
    #[arg(short, long, default_value = "20")]
    leverage: u8,
    #[arg(long)]
    testnet: bool,

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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Position {
    None,
    Long,
    Short,
}

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

fn create_strategy(args: &Args) -> Box<dyn Strategy> {
    match args.strategy.as_str() {
        "macd_rsi" => Box::new(MacdRsiStrategy::new(30.0, 70.0, 3.5, 6.0, 9.0)),
        "ao_candle" => Box::new(AoCandleStrategy::new(5, 34, 3.5, 6.0, 9.0)),
        _ => Box::new(MacdRsiStrategy::new(30.0, 70.0, 3.5, 6.0, 9.0)),
    }
}

async fn get_crypto_price(client: &reqwest::Client, symbol: &str, api_base: &str) -> Result<f64> {
    let url = format!("{}/info", api_base.trim().trim_end_matches('/'));
    let body = serde_json::json!({"type": "allMids"});

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
    let fallback = match symbol {
        "SOLUSDT" => 190.0,
        "ETHUSDT" => 3500.0,
        _ => 95000.0,
    };
    warn!("⚠️ Fallback price ({}): ${:.2}", symbol, fallback);
    Ok(fallback)
}

// === Отправка ордера через наш ExchangeClient ===
// ✅ FIX E0061: вызов market_order(coin, size, is_buy) — ровно 3 аргумента
async fn execute_entry(
    exchange: &ExchangeClient,
    size: f64,
    is_long: bool,
    symbol: &str,
    _current_price: f64,
    _account_equity: f64,
) -> Result<bool> {
    let coin = symbol.replace("USDT", "");

    // ✅ FIX: правильный порядок аргументов: (coin, size, is_buy)
    match exchange.market_order(&coin, size, is_long).await {
        Ok(response) => {
            info!(
                "📈 {} entry sent ({}) | Response: {:?}",
                if is_long { "LONG" } else { "SHORT" },
                symbol,
                response
            );
            Ok(true)
        }
        Err(e) => {
            error!(
                "❌ {} entry failed ({}): {}",
                if is_long { "LONG" } else { "SHORT" },
                symbol,
                e
            );
            Ok(false)
        }
    }
}

// ✅ FIX E0599: нет close_position — используем market_order в обратную сторону
async fn execute_exit(
    exchange: &ExchangeClient,
    size: f64,
    is_long: bool,
    symbol: &str,
    _current_price: f64,
) -> Result<bool> {
    let coin = symbol.replace("USDT", "");

    // ✅ FIX: закрытие = рыночный ордер в ОБРАТНУЮ сторону
    let is_buy_to_close = !is_long;
    match exchange.market_order(&coin, size, is_buy_to_close).await {
        Ok(response) => {
            info!(
                "📉 {} exit sent ({}) | Response: {:?}",
                if is_long { "LONG" } else { "SHORT" },
                symbol,
                response
            );
            Ok(true)
        }
        Err(e) => {
            error!(
                "❌ {} exit failed ({}): {}",
                if is_long { "LONG" } else { "SHORT" },
                symbol,
                e
            );
            Ok(false)
        }
    }
}

async fn run_backtest_mode(args: &Args) -> Result<()> {
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
    let end_time = args
        .to
        .as_ref()
        .map(|s| parse_date(s))
        .unwrap_or_else(|| Ok(Utc::now().timestamp_millis()))?;
    let start_time = args
        .from
        .as_ref()
        .map(|s| parse_date(s))
        .unwrap_or_else(|| Ok((Utc::now() - chrono::Duration::days(7)).timestamp_millis()))?;
    if args.timeframe_sweep {
        info!("🔄 Запуск параметрического поиска по таймфреймам...");
        let strategy = create_strategy(args);
        let results = run_timeframe_sweep(&args.symbol, start_time, end_time, &*strategy).await?;
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
    } else {
        let strategy = create_strategy(args);
        let result = run_backtest(&candles, &*strategy).await?;
        print_single_result(&result);
    }
    Ok(())
}

async fn run_live_mode(args: &Args) -> Result<()> {
    let strategy = create_strategy(args);
    let leverage = args.leverage;

    // === Загрузка private key ===
    let private_key =
        std::env::var("HYPERLIQUID_PRIVATE_KEY").expect("❌ Need HYPERLIQUID_PRIVATE_KEY in .env");

    // === Инициализация торговых клиентов ===
    let exchange = ExchangeClient::new(&private_key, args.testnet)?;

    // ✅ FIX E0609: address находится внутри info_client
    let _info = HyperliquidClient::new(args.testnet, exchange.info_client.address.clone());

    info!("🦖 MonsterFi Executor v0.10.1 started");
    info!(
        "🎯 Strategy: {} | Leverage: {}x | Symbol: {}",
        strategy.name(),
        leverage,
        args.symbol
    );
    // ✅ FIX E0609: exchange.info_client.address
    info!(
        "🔑 Wallet: {} | Testnet: {}",
        exchange
            .info_client
            .address
            .as_ref()
            .unwrap_or(&"Unknown".to_string()),
        args.testnet
    );

    let http_client = reqwest::Client::new();
    let mut position = Position::None;
    let mut price_buffer: VecDeque<f64> = VecDeque::with_capacity(500);
    let mut trailing_stop = TrailingStop::new(strategy.get_trail_pct());
    let mut prev_histogram: Option<f64> = None;
    let mut entry_price: Option<f64> = None;
    let mut prev_rsi: Option<f64> = None;
    let mut ao_values: VecDeque<Option<f64>> = VecDeque::with_capacity(100);
    let mut candle_history: VecDeque<(f64, f64, f64, f64)> = VecDeque::with_capacity(10);
    let mut highs_buffer: VecDeque<f64> = VecDeque::with_capacity(500);
    let mut lows_buffer: VecDeque<f64> = VecDeque::with_capacity(500);
    let mut interval_timer = interval(Duration::from_secs(6));

    loop {
        interval_timer.tick().await;
        let current_price = get_crypto_price(
            &http_client,
            &args.symbol,
            if args.testnet {
                "https://api.hyperliquid-testnet.xyz"
            } else {
                "https://api.hyperliquid.xyz"
            },
        )
        .await?;
        price_buffer.push_back(current_price);
        if price_buffer.len() > 500 {
            price_buffer.pop_front();
        }
        highs_buffer.push_back(current_price);
        lows_buffer.push_back(current_price);
        if highs_buffer.len() > 500 {
            highs_buffer.pop_front();
        }
        if lows_buffer.len() > 500 {
            lows_buffer.pop_front();
        }
        candle_history.push_back((current_price, current_price, current_price, current_price));
        if candle_history.len() > 10 {
            candle_history.pop_front();
        }
        let buffer_len = price_buffer.len();

        if buffer_len >= 35 {
            let prices: Vec<f64> = price_buffer.iter().copied().collect();
            if let Some((histogram, _macd_line)) = calculate_macd_histogram(&prices) {
                let rsi_val = calculate_rsi(&prices, 14).unwrap_or(50.0);
                let ao_val = if highs_buffer.len() >= 34 {
                    let highs: Vec<f64> = highs_buffer.iter().copied().collect();
                    let lows: Vec<f64> = lows_buffer.iter().copied().collect();
                    calculate_ao(&highs, &lows, 5, 34)
                } else {
                    None
                };
                ao_values.push_back(ao_val);
                if ao_values.len() > 100 {
                    ao_values.pop_front();
                }

                let ctx = IndicatorContext {
                    rsi: rsi_val,
                    prev_rsi,
                    macd_histogram: histogram,
                    prev_macd_histogram: prev_histogram,
                    macd_line: 0.0,
                    ao: ao_val,
                    prev_ao: ao_values
                        .get(ao_values.len().saturating_sub(2))
                        .and_then(|&x| x),
                    prev_prev_ao: ao_values
                        .get(ao_values.len().saturating_sub(3))
                        .and_then(|&x| x),
                    prev_prev_prev_ao: ao_values
                        .get(ao_values.len().saturating_sub(4))
                        .and_then(|&x| x),
                    current_candle: candle_history.back().copied(),
                    prev_candle: candle_history.iter().rev().nth(1).copied(),
                    prev_prev_candle: candle_history.iter().rev().nth(2).copied(),
                };

                if let (Some(a0), Some(a1), Some(a2), Some(a3)) =
                    (ctx.ao, ctx.prev_ao, ctx.prev_prev_ao, ctx.prev_prev_prev_ao)
                {
                    let g2 = a2 > a3;
                    let g1 = a1 > a2;
                    let g0 = a0 > a1;
                    let sell = g2 && !g1 && !g0 && a2 > 0.0 && a1 > 0.0 && a0 > 0.0;
                    let buy = !g2 && g1 && g0 && a2 < 0.0 && a1 < 0.0 && a0 < 0.0;
                    info!("🔍 AO Pattern | [-2]: {:.2}({}) | [-1]: {:.2}({}) | [0]: {:.2}({}) | SELL: {} | BUY: {}",
                        a2,if g2{"🟢"}else{"🔴"}, a1,if g1{"🟢"}else{"🔴"}, a0,if g0{"🟢"}else{"🔴"}, sell, buy);
                }

                info!(
                    "📊 MACD: {:.4} | RSI: {:.1} | AO: {:?} | Pos: {:?}",
                    histogram, rsi_val, ao_val, position
                );
                let signal = strategy.generate_signal(&ctx);
                info!("🔍 Entry | Signal: {:?}", signal);

                if position == Position::None {
                    match signal {
                        Signal::Long => {
                            info!(
                                "🚀 SIGNAL LONG | AO: {:?} | Price: ${:.2}",
                                ao_val, current_price
                            );
                            match execute_entry(
                                &exchange,
                                0.001,
                                true,
                                &args.symbol,
                                current_price,
                                1000.0,
                            )
                            .await
                            {
                                Ok(true) => {
                                    position = Position::Long;
                                    entry_price = Some(current_price);
                                    trailing_stop.reset();
                                    trailing_stop.highest_price = Some(current_price);
                                    info!("✅ Position: Long @ ${}", current_price);
                                }
                                Ok(false) => warn!("⚠️ Order failed"),
                                Err(e) => error!("❌ Error: {}", e),
                            }
                        }
                        Signal::Short => {
                            info!(
                                "🚀 SIGNAL SHORT | AO: {:?} | Price: ${:.2}",
                                ao_val, current_price
                            );
                            match execute_entry(
                                &exchange,
                                0.001,
                                false,
                                &args.symbol,
                                current_price,
                                1000.0,
                            )
                            .await
                            {
                                Ok(true) => {
                                    position = Position::Short;
                                    entry_price = Some(current_price);
                                    trailing_stop.reset();
                                    trailing_stop.lowest_price = Some(current_price);
                                    info!("✅ Position: Short @ ${}", current_price);
                                }
                                Ok(false) => warn!("⚠️ Order failed"),
                                Err(e) => error!("❌ Error: {}", e),
                            }
                        }
                        Signal::Hold => {}
                    }
                }

                if position != Position::None {
                    let mut exit = false;
                    let is_long = position == Position::Long;
                    let sl = entry_price.unwrap()
                        * (1.0
                            - if is_long { 1.0 } else { -1.0 } * strategy.get_max_loss_pct()
                                / 100.0
                                / leverage as f64);
                    if (is_long && current_price <= sl) || (!is_long && current_price >= sl) {
                        exit = true;
                        info!("🛑 Stop-loss @ ${:.2}", current_price);
                    }
                    if !exit {
                        if let Some(sp) = trailing_stop.update(current_price, position) {
                            exit = true;
                            info!("🛑 Trailing @ ${:.2}", sp);
                        }
                    }
                    if !exit {
                        let pnl = if is_long {
                            (current_price - entry_price.unwrap()) / entry_price.unwrap() * 100.0
                        } else {
                            (entry_price.unwrap() - current_price) / entry_price.unwrap() * 100.0
                        };
                        if pnl >= strategy.get_take_profit_pct() {
                            exit = true;
                            info!("✅ TP +{:.1}%", pnl);
                        }
                    }
                    if exit {
                        match execute_exit(&exchange, 0.01, is_long, &args.symbol, current_price)
                            .await
                        {
                            Ok(true) => {
                                let pnl = if is_long {
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
                                    "📊 TRADE: {} | ${:.2}→${:.2} | PnL: {:+.2}%",
                                    if is_long { "LONG" } else { "SHORT" },
                                    entry_price.unwrap(),
                                    current_price,
                                    pnl
                                );
                                position = Position::None;
                                entry_price = None;
                                trailing_stop.reset();
                            }
                            Ok(false) => warn!("⚠️ Exit failed"),
                            Err(e) => error!("❌ Exit error: {}", e),
                        }
                    }
                }
                prev_histogram = Some(histogram);
                prev_rsi = Some(rsi_val);
            }
        }
        let pnl = if position != Position::None && entry_price.is_some() {
            let e = entry_price.unwrap();
            let p = match position {
                Position::Long => (current_price - e) / e * 100.0,
                Position::Short => (e - current_price) / e * 100.0,
                _ => 0.0,
            };
            format!(" | PnL: {:+.2}%", p)
        } else {
            String::new()
        };
        info!(
            "💓 Heartbeat | Pos: {:?} | ${:.2} | Buffer: {}/500{}",
            position, current_price, buffer_len, pnl
        );
    }
}

// ✅ ЕДИНСТВЕННАЯ функция main — удалена дублирующая в начале файла
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
