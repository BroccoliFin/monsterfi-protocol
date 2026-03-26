// === agent/src/backtest.rs ===
use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use tabled::{Table, Tabled};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct BacktestResult {
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub win_rate: f64,
    pub total_return_pct: f64,
    pub max_drawdown_pct: f64,
    pub avg_trade_return_pct: f64,
    pub sharpe_ratio: f64,
    pub total_commissions_pct: f64,
    pub trades: Vec<TradeLog>,
}

#[derive(Debug, Serialize, Clone)]
pub struct TradeLog {
    pub entry_time: i64,
    pub exit_time: i64,
    pub entry_price: f64,
    pub exit_price: f64,
    pub direction: String,
    pub pnl_pct: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StrategyParams {
    pub rsi_period: usize,
    pub rsi_oversold: f64,
    pub rsi_overbought: f64,
    pub trail_pct: f64,
    pub take_profit_pct: f64,
    pub leverage: u8,
    #[allow(dead_code)]
    pub position_size: f64,
    pub max_loss_pct: f64,
}

impl Default for StrategyParams {
    fn default() -> Self {
        Self {
            rsi_period: 14,
            rsi_oversold: 47.0,
            rsi_overbought: 53.0,
            trail_pct: 7.0,
            take_profit_pct: 11.0,
            leverage: 10,
            position_size: 0.01,
            max_loss_pct: 6.0,
        }
    }
}

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

#[derive(Debug, Clone)]
pub struct TrailingStop {
    pub highest_price: Option<f64>,
    pub lowest_price: Option<f64>,
    pub trail_pct: f64,
}

impl TrailingStop {
    pub fn new(trail_pct: f64) -> Self {
        Self {
            highest_price: None,
            lowest_price: None,
            trail_pct,
        }
    }
    pub fn reset(&mut self) {
        self.highest_price = None;
        self.lowest_price = None;
    }
    pub fn update(&mut self, price: f64, is_long: bool) -> Option<f64> {
        if is_long {
            if self.highest_price.is_none() || price > self.highest_price.unwrap() {
                self.highest_price = Some(price);
            }
            let stop = self.highest_price.unwrap() * (1.0 - self.trail_pct / 100.0);
            if price <= stop {
                Some(stop)
            } else {
                None
            }
        } else {
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
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Position {
    None,
    Long,
    Short,
}

pub async fn fetch_binance_klines(
    symbol: &str,
    interval: &str,
    start_time: i64,
    end_time: i64,
    limit: usize,
) -> Result<Vec<Candle>> {
    let binance_interval = match interval {
        "1m" | "m1" | "1" => "1m",
        "3m" | "m3" | "3" => "3m",
        "5m" | "m5" | "5" => "5m",
        "15m" | "m15" | "15" => "15m",
        "30m" | "m30" | "30" => "30m",
        "1h" | "h1" | "60" => "1h",
        "2h" | "h2" | "120" => "2h",
        "4h" | "h4" | "240" => "4h",
        "6h" | "h6" | "360" => "6h",
        "12h" | "h12" | "720" => "12h",
        "1d" | "d1" => "1d",
        "3d" | "d3" => "3d",
        "1w" | "w1" => "1w",
        _ => "1m",
    };

    let client = Client::new();
    let mut all_candles = Vec::new();
    let mut current_start = start_time;

    info!(
        "🔍 DEBUG: Fetching {} [{}] from {} to {}",
        symbol,
        binance_interval,
        DateTime::from_timestamp_millis(start_time).unwrap(),
        DateTime::from_timestamp_millis(end_time).unwrap()
    );

    while current_start < end_time && all_candles.len() < limit {
        // ✅ ИСПРАВЛЕНО: убраны лишние пробелы после symbol=
        let url = format!(
            "https://api.binance.com/api/v3/klines?symbol={}&interval={}&startTime={}&endTime={}&limit=1000",
            symbol, binance_interval, current_start, end_time
        );

        let resp = client.get(&url).send().await?;
        if !resp.status().is_success() {
            warn!("⚠️ Binance API error: {}", resp.status());
            break;
        }

        let data: serde_json::Value = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| anyhow::anyhow!("JSON parse error: {}", e))?;

        let candles_array = data
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid Binance response"))?;

        for candle_data in candles_array {
            if let Some(arr) = candle_data.as_array() {
                if arr.len() >= 6 {
                    let timestamp = arr[0].as_i64().unwrap_or(0);
                    let candle = Candle {
                        timestamp,
                        open: arr[1].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                        high: arr[2].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                        low: arr[3].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                        close: arr[4].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                        volume: arr[5].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                    };
                    all_candles.push(candle);

                    let interval_ms = match binance_interval {
                        "1m" => 60_000,
                        "3m" => 180_000,
                        "5m" => 300_000,
                        "15m" => 900_000,
                        "30m" => 1_800_000,
                        "1h" => 3_600_000,
                        "2h" => 7_200_000,
                        "4h" => 14_400_000,
                        "6h" => 21_600_000,
                        "12h" => 43_200_000,
                        "1d" => 86_400_000,
                        "3d" => 259_200_000,
                        "1w" => 604_800_000,
                        _ => 60_000,
                    };
                    current_start = timestamp + interval_ms;
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    if let (Some(first), Some(last)) = (all_candles.first(), all_candles.last()) {
        let duration_hours = (last.timestamp - first.timestamp) as f64 / 3_600_000.0;
        let actual_interval_sec = if all_candles.len() > 1 {
            (last.timestamp - first.timestamp) / all_candles.len() as i64 / 1000
        } else {
            0
        };

        info!("🔍 DEBUG: Loaded {} candles", all_candles.len());
        info!(
            "🔍 DEBUG: First: {} @ ${:.2} | Last: {} @ ${:.2}",
            DateTime::from_timestamp_millis(first.timestamp).unwrap(),
            first.close,
            DateTime::from_timestamp_millis(last.timestamp).unwrap(),
            last.close
        );
        info!(
            "🔍 DEBUG: Period: {:.2}h | Actual interval: {}s | Expected: {}",
            duration_hours, actual_interval_sec, binance_interval
        );

        if binance_interval == "1m" && actual_interval_sec > 120 {
            warn!(
                "⚠️ WARNING: Requested 1m but got ~{}s candles!",
                actual_interval_sec
            );
        }
    }
    Ok(all_candles)
}

pub async fn run_backtest(candles: &[Candle], params: &StrategyParams) -> Result<BacktestResult> {
    if candles.len() < 50 {
        return Err(anyhow::anyhow!("Not enough candles"));
    }

    let mut price_buffer: VecDeque<f64> = VecDeque::with_capacity(500);
    let mut trailing_stop = TrailingStop::new(params.trail_pct);
    let mut position = Position::None;
    let mut entry_price = 0.0;
    let mut prev_histogram: Option<f64> = None;

    let mut balance = 1.0;
    let mut peak_balance = 1.0;
    let mut max_drawdown = 0.0;
    let mut total_commissions = 0.0;
    let mut trades = Vec::new();
    let mut trade_returns = Vec::new();
    let mut entry_time: i64 = 0;

    let mut current_price = 0.0;

    for (idx, candle) in candles.iter().enumerate() {
        current_price = candle.close;
        price_buffer.push_back(current_price);
        if price_buffer.len() > 500 {
            price_buffer.pop_front();
        }

        if price_buffer.len() >= 35 {
            let prices: Vec<f64> = price_buffer.iter().copied().collect();

            if let Some((histogram, _macd_line)) = macd_histogram(&prices) {
                let rsi_val = rsi(&prices, params.rsi_period).unwrap_or(50.0);

                if idx % 1000 == 0 && idx > 0 {
                    let _prev = prev_histogram.unwrap_or(0.0);
                    info!(
                        "🔍 [{}] MACD: {:.4} | RSI: {:.1} | Price: ${:.2}",
                        idx, histogram, rsi_val, current_price
                    );
                }

                if position == Position::None {
                    let prev = prev_histogram.unwrap_or(0.0);
                    let bullish_cross = prev <= 0.0 && histogram > 0.0;
                    let bearish_cross = prev >= 0.0 && histogram < 0.0;
                    let oversold = rsi_val < params.rsi_oversold;
                    let overbought = rsi_val > params.rsi_overbought;

                    if idx % 500 == 0 {
                        info!("🔍 Entry | BullishCross: {} | Oversold: {} | BearishCross: {} | Overbought: {}",
                              bullish_cross, oversold, bearish_cross, overbought);
                    }

                    if bullish_cross && oversold {
                        let ts = DateTime::from_timestamp_millis(candle.timestamp).unwrap();
                        info!(
                            "🚀 SIGNAL LONG @ {} | MACD: {:.4} | RSI: {:.1} | Price: ${:.2}",
                            ts.format("%m-%d %H:%M:%S"),
                            histogram,
                            rsi_val,
                            current_price
                        );
                        position = Position::Long;
                        entry_price = current_price;
                        entry_time = candle.timestamp;
                        trailing_stop.reset();
                        trailing_stop.highest_price = Some(current_price);
                    } else if bearish_cross && overbought {
                        let ts = DateTime::from_timestamp_millis(candle.timestamp).unwrap();
                        info!(
                            "🚀 SIGNAL SHORT @ {} | MACD: {:.4} | RSI: {:.1} | Price: ${:.2}",
                            ts.format("%m-%d %H:%M:%S"),
                            histogram,
                            rsi_val,
                            current_price
                        );
                        position = Position::Short;
                        entry_price = current_price;
                        entry_time = candle.timestamp;
                        trailing_stop.reset();
                        trailing_stop.lowest_price = Some(current_price);
                    }
                }

                if position != Position::None {
                    let mut exit_reason = String::new();
                    let mut should_exit = false;
                    let is_long = position == Position::Long;

                    let price_stop_loss = if is_long {
                        entry_price * (1.0 - params.max_loss_pct / 100.0 / params.leverage as f64)
                    } else {
                        entry_price * (1.0 + params.max_loss_pct / 100.0 / params.leverage as f64)
                    };

                    if (is_long && current_price <= price_stop_loss)
                        || (!is_long && current_price >= price_stop_loss)
                    {
                        should_exit = true;
                        exit_reason = "hard_stop_loss".to_string();
                        info!(
                            "🛑 HARD STOP-LOSS @ ${:.2} (entry: ${:.2}, loss: -{:.2}%)",
                            current_price, entry_price, params.max_loss_pct
                        );
                    }

                    if !should_exit {
                        if let Some(stop_price) = trailing_stop.update(current_price, is_long) {
                            should_exit = true;
                            exit_reason = "trailing_stop".to_string();
                            info!("🛑 Trailing stop @ ${:.2}", stop_price);
                        }
                    }

                    if !should_exit {
                        let raw_pnl = if is_long {
                            (current_price - entry_price) / entry_price * 100.0
                        } else {
                            (entry_price - current_price) / entry_price * 100.0
                        };
                        if raw_pnl >= params.take_profit_pct {
                            should_exit = true;
                            exit_reason = "take_profit".to_string();
                            info!("✅ Take-profit: +{:.1}%", raw_pnl);
                        }
                    }

                    if should_exit {
                        let raw_pnl_pct = if position == Position::Long {
                            (current_price - entry_price) / entry_price * 100.0
                        } else {
                            (entry_price - current_price) / entry_price * 100.0
                        };

                        let leveraged_pnl = raw_pnl_pct * params.leverage as f64;
                        let commission = 0.08;
                        let pnl_pct = leveraged_pnl - commission;

                        let entry_ts = DateTime::from_timestamp_millis(entry_time).unwrap();
                        let exit_ts = DateTime::from_timestamp_millis(candle.timestamp).unwrap();
                        info!("📊 TRADE: {} | Entry: {} @ ${:.2} | Exit: {} @ ${:.2} | PnL: {:+.2}% ({})",
                              if position==Position::Long{"LONG"}else{"SHORT"},
                              entry_ts.format("%m-%d %H:%M"), entry_price,
                              exit_ts.format("%m-%d %H:%M"), current_price, pnl_pct, exit_reason);

                        total_commissions += commission;
                        balance *= 1.0 + pnl_pct / 100.0;
                        if balance > peak_balance {
                            peak_balance = balance;
                        }
                        let drawdown = (peak_balance - balance) / peak_balance * 100.0;
                        if drawdown > max_drawdown {
                            max_drawdown = drawdown;
                        }

                        trades.push(TradeLog {
                            entry_time,
                            exit_time: candle.timestamp,
                            entry_price,
                            exit_price: current_price,
                            direction: if position == Position::Long {
                                "Long"
                            } else {
                                "Short"
                            }
                            .to_string(),
                            pnl_pct,
                            reason: exit_reason.clone(),
                        });
                        trade_returns.push(pnl_pct);
                        position = Position::None;
                        entry_price = 0.0;
                    }
                }
                prev_histogram = Some(histogram);
            }
        }
    }

    if position != Position::None {
        let raw_pnl_pct = if position == Position::Long {
            (current_price - entry_price) / entry_price * 100.0
        } else {
            (entry_price - current_price) / entry_price * 100.0
        };

        let leveraged_pnl = raw_pnl_pct * params.leverage as f64;
        let commission = 0.08;
        let pnl_pct = leveraged_pnl - commission;

        let entry_ts = DateTime::from_timestamp_millis(entry_time).unwrap();
        let exit_ts = DateTime::from_timestamp_millis(candles.last().unwrap().timestamp).unwrap();
        info!("🔚 FORCE CLOSE | {} | Entry: {} @ ${:.2} | Exit: {} @ ${:.2} | PnL: {:+.2}% (end of period)",
              if position==Position::Long{"LONG"}else{"SHORT"},
              entry_ts.format("%m-%d %H:%M"), entry_price,
              exit_ts.format("%m-%d %H:%M"), current_price, pnl_pct);

        total_commissions += commission;
        balance *= 1.0 + pnl_pct / 100.0;

        trades.push(TradeLog {
            entry_time,
            exit_time: candles.last().unwrap().timestamp,
            entry_price,
            exit_price: current_price,
            direction: if position == Position::Long {
                "Long"
            } else {
                "Short"
            }
            .to_string(),
            pnl_pct,
            reason: "end_of_period".to_string(),
        });
        trade_returns.push(pnl_pct);
    }

    info!(
        "🔍 DEBUG: Total candles: {} | Trades executed: {}",
        candles.len(),
        trades.len()
    );

    let total_trades = trades.len();
    let winning_trades = trades.iter().filter(|t| t.pnl_pct > 0.0).count();
    let win_rate = if total_trades > 0 {
        winning_trades as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };
    let total_return_pct = (balance - 1.0) * 100.0;
    let avg_trade_return_pct = if !trade_returns.is_empty() {
        trade_returns.iter().sum::<f64>() / trade_returns.len() as f64
    } else {
        0.0
    };
    let sharpe_ratio = if !trade_returns.is_empty() {
        let mean = trade_returns.iter().sum::<f64>() / trade_returns.len() as f64;
        let variance = trade_returns
            .iter()
            .map(|&r| (r - mean).powi(2))
            .sum::<f64>()
            / trade_returns.len() as f64;
        let std_dev = variance.sqrt();
        if std_dev > 0.0 {
            mean / std_dev
        } else {
            0.0
        }
    } else {
        0.0
    };

    Ok(BacktestResult {
        total_trades,
        winning_trades,
        losing_trades: total_trades - winning_trades,
        win_rate,
        total_return_pct,
        max_drawdown_pct: max_drawdown,
        avg_trade_return_pct,
        sharpe_ratio,
        total_commissions_pct: total_commissions,
        trades,
    })
}

#[allow(dead_code)]
pub fn export_results_json(
    result: &BacktestResult,
    params: &StrategyParams,
    filepath: &str,
) -> Result<()> {
    #[derive(Serialize)]
    struct ExportData {
        params: StrategyParams,
        result: BacktestResult,
        timestamp: String,
    }

    let data = ExportData {
        params: params.clone(),
        result: result.clone(),
        timestamp: Utc::now().to_rfc3339(),
    };

    let json = serde_json::to_string_pretty(&data)?;
    let mut file = File::create(filepath)?;
    file.write_all(json.as_bytes())?;

    info!("💾 Exported results to {}", filepath);
    Ok(())
}

#[allow(dead_code)]
pub fn export_trades_csv(trades: &[TradeLog], filepath: &str) -> Result<()> {
    let mut wtr = csv::Writer::from_path(filepath)?;

    for trade in trades {
        wtr.serialize(trade)?;
    }

    wtr.flush()?;
    info!("💾 Exported {} trades to {}", trades.len(), filepath);
    Ok(())
}

#[derive(Debug, Clone)]
pub struct TimeframeResult {
    pub timeframe: String,
    pub candles_loaded: usize,
    pub result: BacktestResult,
}

pub async fn run_timeframe_sweep(
    symbol: &str,
    start_time: i64,
    end_time: i64,
    params: &StrategyParams,
) -> Result<Vec<TimeframeResult>> {
    let timeframes = ["1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w"];
    let mut results = Vec::new();

    println!("\n🔄 Запуск параметрического поиска по таймфреймам...");
    println!(
        "📅 Период: {} → {}\n",
        DateTime::from_timestamp_millis(start_time).unwrap(),
        DateTime::from_timestamp_millis(end_time).unwrap()
    );

    for tf in timeframes {
        print!("⏳ Тестируем {}... ", tf);

        match fetch_binance_klines(symbol, tf, start_time, end_time, 100000).await {
            Ok(candles) => match run_backtest(&candles, params).await {
                Ok(result) => {
                    results.push(TimeframeResult {
                        timeframe: tf.to_string(),
                        candles_loaded: candles.len(),
                        result: result.clone(),
                    });
                    println!(
                        "✅ {} свечей, {} сделок",
                        candles.len(),
                        result.total_trades
                    );
                }
                Err(e) => {
                    warn!("⚠️ {} skipped: {}", tf, e);
                    results.push(TimeframeResult {
                        timeframe: tf.to_string(),
                        candles_loaded: candles.len(),
                        result: BacktestResult {
                            total_trades: 0,
                            winning_trades: 0,
                            losing_trades: 0,
                            win_rate: 0.0,
                            total_return_pct: 0.0,
                            max_drawdown_pct: 0.0,
                            avg_trade_return_pct: 0.0,
                            sharpe_ratio: f64::NEG_INFINITY,
                            total_commissions_pct: 0.0,
                            trades: Vec::new(),
                        },
                    });
                    println!("⚠️  Пропущено ({} свечей)", candles.len());
                }
            },
            Err(e) => {
                warn!("⚠️ {} fetch failed: {}", tf, e);
                results.push(TimeframeResult {
                    timeframe: tf.to_string(),
                    candles_loaded: 0,
                    result: BacktestResult {
                        total_trades: 0,
                        winning_trades: 0,
                        losing_trades: 0,
                        win_rate: 0.0,
                        total_return_pct: 0.0,
                        max_drawdown_pct: 0.0,
                        avg_trade_return_pct: 0.0,
                        sharpe_ratio: f64::NEG_INFINITY,
                        total_commissions_pct: 0.0,
                        trades: Vec::new(),
                    },
                });
                println!("❌ Ошибка загрузки");
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    results.sort_by(|a, b| {
        b.result
            .sharpe_ratio
            .partial_cmp(&a.result.sharpe_ratio)
            .unwrap()
    });

    Ok(results)
}

#[allow(dead_code)]
pub fn print_timeframe_comparison(results: &[TimeframeResult]) {
    println!("\n");
    println!("🏆 СРАВНЕНИЕ ТАЙМФРЕЙМОВ (отсортировано по Sharpe Ratio)");
    println!("{}", "═".repeat(120));

    println!(
        "{:<8} {:>10} {:>8} {:>8} {:>12} {:>12} {:>12} {:>10} {:>10}",
        "Rank", "TF", "Candles", "Trades", "Win Rate", "Return", "Max DD", "Sharpe", "Commissions"
    );
    println!("{}", "─".repeat(120));

    for (i, r) in results.iter().enumerate() {
        // ✅ Проверяем, есть ли валидные данные
        let has_data = r.result.sharpe_ratio.is_finite() && r.result.total_trades > 0;

        let rank = if has_data {
            if i == 0 {
                "🥇"
            } else if i == 1 {
                "🥈"
            } else if i == 2 {
                "🥉"
            } else {
                &format!("{}", i + 1)
            }
        } else {
            "—"
        };

        // ✅ Форматируем значения: "—" если нет данных
        let trades_str = if has_data {
            format!("{}", r.result.total_trades)
        } else {
            "—".to_string()
        };
        let win_rate_str = if has_data {
            format!("{:.1}%", r.result.win_rate)
        } else {
            "—".to_string()
        };
        let return_str = if has_data {
            format!("{:+.2}%", r.result.total_return_pct)
        } else {
            "—".to_string()
        };
        let dd_str = if has_data {
            format!("-{:.2}%", r.result.max_drawdown_pct)
        } else {
            "—".to_string()
        };
        let sharpe_str = if has_data {
            format!("{:.2}", r.result.sharpe_ratio)
        } else {
            "—".to_string()
        };
        let comm_str = if has_data {
            format!("{:.2}%", r.result.total_commissions_pct)
        } else {
            "—".to_string()
        };

        println!(
            "{:<8} {:>10} {:>8} {:>8} {:>12} {:>12} {:>12} {:>10} {:>10}",
            rank,
            r.timeframe,
            r.candles_loaded,
            trades_str,
            win_rate_str,
            return_str,
            dd_str,
            sharpe_str,
            comm_str
        );
    }

    println!("{}", "═".repeat(120));

    // ✅ Показываем рекомендацию только если есть валидные результаты
    if let Some(best) = results.iter().find(|r| r.result.sharpe_ratio.is_finite()) {
        println!(
            "\n✅ РЕКОМЕНДАЦИЯ: {} показывает лучший Sharpe Ratio ({:.2})",
            best.timeframe, best.result.sharpe_ratio
        );
        println!("   → Используйте этот таймфрейм для продакшена");
    } else {
        println!("\n⚠️  Нет валидных результатов для рекомендации");
    }

    println!("\n");
}

pub struct ParamSweep {
    pub rsi_oversold_range: Vec<f64>,
    pub rsi_overbought_range: Vec<f64>,
    pub trail_pct_range: Vec<f64>,
    pub take_profit_range: Vec<f64>,
    pub leverage_range: Vec<u8>,
}
impl Default for ParamSweep {
    fn default() -> Self {
        Self {
            rsi_oversold_range: vec![30.0, 35.0, 40.0, 45.0],
            rsi_overbought_range: vec![55.0, 60.0, 65.0, 70.0],
            trail_pct_range: vec![1.0, 1.5, 2.0, 2.5],
            take_profit_range: vec![3.0, 5.0, 7.0, 10.0],
            leverage_range: vec![10, 20],
        }
    }
}
pub async fn run_param_sweep(
    candles: &[Candle],
    sweep: &ParamSweep,
) -> Result<Vec<(StrategyParams, BacktestResult)>> {
    let mut results = Vec::new();
    let base = StrategyParams::default();
    for &ro in &sweep.rsi_oversold_range {
        for &rb in &sweep.rsi_overbought_range {
            for &tp in &sweep.trail_pct_range {
                for &tf in &sweep.take_profit_range {
                    for &lv in &sweep.leverage_range {
                        let mut p = base.clone();
                        p.rsi_oversold = ro;
                        p.rsi_overbought = rb;
                        p.trail_pct = tp;
                        p.take_profit_pct = tf;
                        p.leverage = lv;
                        if let Ok(r) = run_backtest(candles, &p).await {
                            results.push((p, r));
                        }
                    }
                }
            }
        }
    }
    results.sort_by(|a, b| {
        b.1.total_return_pct
            .partial_cmp(&a.1.total_return_pct)
            .unwrap()
    });
    Ok(results)
}

#[derive(Tabled)]
struct Row {
    rank: usize,
    ro: f64,
    rb: f64,
    tp: f64,
    tf: f64,
    lv: u8,
    trades: usize,
    wr: String,
    ret: String,
    dd: String,
    sp: String,
}
pub fn print_results_table(results: &[(StrategyParams, BacktestResult)], top_n: usize) {
    let rows: Vec<Row> = results
        .iter()
        .take(top_n)
        .enumerate()
        .map(|(i, (p, r))| Row {
            rank: i + 1,
            ro: p.rsi_oversold,
            rb: p.rsi_overbought,
            tp: p.trail_pct,
            tf: p.take_profit_pct,
            lv: p.leverage,
            trades: r.total_trades,
            wr: format!("{:.1}%", r.win_rate),
            ret: format!("{:+.2}%", r.total_return_pct),
            dd: format!("-{:.2}%", r.max_drawdown_pct),
            sp: format!("{:.2}", r.sharpe_ratio),
        })
        .collect();
    println!("\n🏆 Top {}:\n{}\n", top_n, Table::new(rows));
}

pub fn print_single_result(r: &BacktestResult) {
    println!("\n📊 Backtest Results:");
    println!("  ─────────────────────────────────────────");
    println!(
        "  Trades: {} | Win: {:.1}% | Return: {:+.2}% | DD: -{:.2}% | Sharpe: {:.2}",
        r.total_trades, r.win_rate, r.total_return_pct, r.max_drawdown_pct, r.sharpe_ratio
    );
    println!("  Commissions: -{:.2}%", r.total_commissions_pct);
    println!("  ─────────────────────────────────────────");
    if !r.trades.is_empty() {
        println!("\n📈 Trades:");
        for t in &r.trades {
            let e = DateTime::from_timestamp_millis(t.entry_time)
                .unwrap()
                .format("%m-%d %H:%M");
            let x = DateTime::from_timestamp_millis(t.exit_time)
                .unwrap()
                .format("%m-%d %H:%M");
            println!(
                "  {} {}→{}: {:+.2}% ({}) | ${:.2}→${:.2}",
                t.direction, e, x, t.pnl_pct, t.reason, t.entry_price, t.exit_price
            );
        }
    }
}
