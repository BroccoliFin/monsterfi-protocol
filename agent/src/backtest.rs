// === agent/src/backtest.rs ===

pub async fn run_backtest(
    api_base: &str,
    symbol: &str,
    timeframe: &str, // "1m" or "5m"
    start_timestamp: u64,
    end_timestamp: u64,
) -> Result<BacktestResult, anyhow::Error> {
    
    // Загружаем исторические свечи (упрощённо)
    let prices = fetch_historical_prices(api_base, symbol, timeframe, start_timestamp, end_timestamp).await?;
    
    let mut strategy = MACD_RSIStrategy::new(20);
    let mut balance = 1.0; // 1 BTC starting
    let mut trades = Vec::new();
    
    for (i, &price) in prices.iter().enumerate() {
        if let Some(signal) = strategy.evaluate(&prices[..=i], price) {
            match signal {
                Signal::EnterLong { entry_price, size, .. } => {
                    // Симуляция входа
                    trades.push(Trade { entry: entry_price, exit: None, size });
                }
                Signal::Exit { exit_price, .. } => {
                    if let Some(last) = trades.last_mut() {
                        last.exit = Some(exit_price);
                        let pnl = (exit_price - last.entry) * last.size;
                        balance += pnl;
                    }
                }
            }
        }
    }
    
    Ok(BacktestResult {
        initial_balance: 1.0,
        final_balance: balance,
        total_return_pct: (balance - 1.0) / 1.0 * 100.0,
        trades: trades.len(),
        win_rate: calculate_win_rate(&trades),
    })
}