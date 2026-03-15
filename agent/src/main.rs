// MonsterFi Claw — Trading Agent for Hyperliquid

use anyhow::Result;
use dotenvy::dotenv;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use clap::Parser;

use ethers::prelude::*;
use ethers::providers::{Http, Provider};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "fox")]
    strategy: String,
    
    #[arg(short, long, default_value = "20")]
    leverage: u8,
    
    #[arg(long, default_value = "true")]
    testnet: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    
    info!("🦖 MonsterFi Claw starting...");
    dotenv().ok();
    
    let args = Args::parse();
    info!("🎯 Strategy: {} | Leverage: {}x", args.strategy, args.leverage);
    
    let rpc_url = std::env::var("HYPERLIQUID_RPC_URL")
        .unwrap_or_else(|_| "https://api.hyperliquid-testnet.xyz/evm".to_string());
    
    let private_key = std::env::var("PRIVATE_KEY")
        .map_err(|_| anyhow::anyhow!("PRIVATE_KEY not set in .env"))?;
    
    info!("📡 RPC: {}", rpc_url);
    
    // === HyperEVM Provider ===
    match Provider::<Http>::try_from(&rpc_url) {
        Ok(provider) => {
            match provider.get_chainid().await {
                Ok(chain_id) => info!("✅ Connected to HyperEVM (Chain ID: {})", chain_id),
                Err(e) => warn!("⚠️ Could not get chain ID: {}", e),
            }
        }
        Err(e) => warn!("⚠️ Could not connect to HyperEVM: {}", e),
    }
    
    // === HyperLiquid API Check (НЕФАТАЛЬНЫЙ) ===
    let hl_base = if args.testnet {
        "https://api.hyperliquid-testnet.xyz"
    } else {
        "https://api.hyperliquid.xyz"
    };
    
    info!("🔗 HyperLiquid API base: {}", hl_base);
    
    let client = reqwest::Client::new();
    let info_url = format!("{}/info", hl_base);
    
    // Пробуем POST запрос
    let all_mids_body = serde_json::json!({"type": "allMids"});
    
    match client.post(&info_url).json(&all_mids_body).send().await {
        Ok(resp) => {
            let status = resp.status();
            info!("📡 API response status: {}", status);
            
            match resp.text().await {
                Ok(text) => {
                    if text.is_empty() {
                        warn!("⚠️ API returned empty response (testnet may be down)");
                    } else {
                        info!("✅ API response received ({} bytes)", text.len());
                        // Пробуем распарсить как JSON
                        match serde_json::from_str::<serde_json::Value>(&text) {
                            Ok(data) => {
                                let count = data.as_object().map(|o| o.len()).unwrap_or(0);
                                info!("✅ HyperLiquid API reachable ({} assets)", count);
                            }
                            Err(e) => warn!("⚠️ Response is not valid JSON: {}", e),
                        }
                    }
                }
                Err(e) => warn!("⚠️ Could not read response body: {}", e),
            }
        }
        Err(e) => {
            warn!("⚠️ HyperLiquid API unreachable (testnet may be down): {}", e);
            info!("💡 Agent will continue without API connection");
        }
    }
    
    // === Wallet setup ===
    match private_key.parse::<LocalWallet>() {
        Ok(wallet) => {
            let address = wallet.address();
            info!("🔑 Wallet: 0x{}...", hex::encode(&address.as_bytes()[..4]));
        }
        Err(e) => warn!("⚠️ Invalid PRIVATE_KEY format: {}", e),
    }
    
    info!("🚀 Agent ready. Waiting for signals...");
    
    // === Main loop ===
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
    loop {
        interval.tick().await;
        info!("💓 Heartbeat | Strategy: {} | Leverage: {}x", args.strategy, args.leverage);
    }
}
