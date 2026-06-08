// src/trading/exchange.rs
// Hyperliquid Exchange API client — Order signing & execution

use anyhow::{anyhow, Result};
use ethers::signers::{LocalWallet, Signer};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

use super::client::HyperliquidClient;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Нормализует имя монеты: "SOLUSDT" → "SOL", "ethusd" → "ETH"
fn normalize_coin(coin: &str) -> String {
    coin.trim_end_matches("USDT")
        .trim_end_matches("USD")
        .trim_end_matches("PERP")
        .to_uppercase()
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OrderResult {
    pub data: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OrderResponse {
    pub status: String,
    #[serde(rename = "response")]
    pub response: Option<Value>,
}

// ── Exchange Client ──────────────────────────────────────────────────────────

pub struct ExchangeClient {
    pub http: Client,
    pub wallet: LocalWallet,
    pub base_url: String,
    pub info_client: HyperliquidClient,
}

impl ExchangeClient {
    pub fn new(private_key: &str, testnet: bool) -> Result<Self> {
        let wallet: LocalWallet = private_key.parse()?;
        let address = wallet.address().to_string();

        let base_url = if testnet {
            super::client::TESTNET_API
        } else {
            super::client::MAINNET_API
        };

        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(45))
                .build()?,
            wallet,
            base_url: base_url.to_string(),
            info_client: HyperliquidClient::new(testnet, Some(address)),
        })
    }

    fn hash_action(action: &Value) -> Vec<u8> {
        let action_str = serde_json::to_string(action).expect("Failed to serialize action");
        let hash = ethers::utils::keccak256(action_str.as_bytes());
        hash.to_vec()
    }

    // 🔧 FIX: подпись без "0x" префикса — чистый hex
    pub async fn sign_action(&self, action: &Value) -> Result<String> {
        let hash = Self::hash_action(action);
        let sig = self
            .wallet
            .sign_hash(ethers::types::H256::from_slice(&hash))?;

        let v_normalized = if sig.v >= 27 { sig.v - 27 } else { sig.v };

        // FIX: U256 форматируем через format!, а не to_fixed_bytes()
        let r_hex = format!("{:064x}", sig.r);
        let s_hex = format!("{:064x}", sig.s);
        let v_hex = format!("{:02x}", v_normalized);

        Ok(format!("0x{}{}{}", r_hex, s_hex, v_hex))
    }

    fn current_timestamp_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64
    }

    // ── Order helpers ─────────────────────────────────────────────────────────

    pub async fn market_order(&self, coin: &str, size: f64, is_buy: bool) -> Result<OrderResult> {
        let coin_normalized = normalize_coin(coin);
        let asset_idx = self.info_client.asset_index(&coin_normalized).await?;

        // 🔧 Получаем реальную цену для расчёта slippage
        let current_price = self
            .info_client
            .get_price(&coin_normalized)
            .await
            .unwrap_or(100.0);

        // 🔧 Slippage 5% для имитации market-ордера через Limit + IOC
        let slippage = 0.05;
        let limit_px = if is_buy {
            current_price * (1.0 + slippage)
        } else {
            current_price * (1.0 - slippage)
        };

        // 🔧 Лог, который подтвердит, что новая функция работает
        info!(
            "🎯 Market {} {} @ {:.6} (spot: {:.4}, slippage: 5%)",
            if is_buy { "BUY" } else { "SELL" },
            coin_normalized,
            limit_px,
            current_price
        );

        // 🔧 "p" теперь содержит реальную цену, а не "0"
        let order = json!({
            "a": asset_idx,
            "b": is_buy,
            "p": format!("{:.6}", limit_px),  // ← ← ← КЛЮЧЕВОЙ ФИКС
            "s": format!("{:.6}", size),
            "r": false,
            "t": {"limit": {"tif": "Ioc"}}
        });

        let action = json!({
            "type": "order",
            "orders": [order],
            "grouping": "na"
        });

        self.execute_action(action).await
    }

    pub async fn limit_order(
        &self,
        coin: &str,
        size: f64,
        price: f64,
        is_buy: bool,
    ) -> Result<OrderResult> {
        // 🔧 FIX: нормализуем имя монеты
        let coin_normalized = normalize_coin(coin);
        let asset_idx = self.info_client.asset_index(&coin_normalized).await?;
        info!(
            "✅ Asset index for {} (from {}): {}",
            coin_normalized, coin, asset_idx
        );

        let order = json!({
            "a": asset_idx,
            "b": is_buy,
            "p": format!("{:.5}", price),
            "s": format!("{:.6}", size),
            "r": false,
            "t": {"limit": {"tif": "Gtc"}}
        });

        let action = json!({
            "type": "order",
            "orders": [order],
            "grouping": "na"
        });

        self.execute_action(action).await
    }

    // ── Core execution ────────────────────────────────────────────────────────

    async fn execute_action(&self, action: Value) -> Result<OrderResult> {
        let signature = self.sign_action(&action).await?;
        let nonce = Self::current_timestamp_ms();

        let payload = json!({
            "action": action,
            "nonce": nonce,
            "signature": signature
        });

        // 🔍 DEBUG: Выводим ТОЧНЫЙ JSON перед отправкой
        let payload_str = serde_json::to_string(&payload)?;
        debug!("📦 EXACT PAYLOAD: {}", payload_str);
        debug!("🔑 SIGNATURE: {}", signature);

        let url = format!("{}/exchange", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .body(payload_str) // 🔧 Отправляем как строку для точного контроля
            .send()
            .await?;

        let status = resp.status();
        let body_text = resp.text().await?;
        debug!("📥 RAW RESPONSE: {} {}", status, body_text);

        if !status.is_success() {
            return Err(anyhow!("Exchange API error {}: {}", status, body_text));
        }

        let data: serde_json::Value = serde_json::from_str(&body_text)?;
        Ok(OrderResult { data: Some(data) })
    }

    // ── Convenience wrappers ──────────────────────────────────────────────────

    pub async fn buy_market(&self, coin: &str, size: f64) -> Result<OrderResult> {
        self.market_order(coin, size, true).await
    }

    pub async fn sell_market(&self, coin: &str, size: f64) -> Result<OrderResult> {
        self.market_order(coin, size, false).await
    }

    pub async fn buy_limit(&self, coin: &str, size: f64, price: f64) -> Result<OrderResult> {
        self.limit_order(coin, size, price, true).await
    }

    pub async fn sell_limit(&self, coin: &str, size: f64, price: f64) -> Result<OrderResult> {
        self.limit_order(coin, size, price, false).await
    }

    // ── Account helpers ───────────────────────────────────────────────────────

    pub async fn get_account_value(&self) -> Result<f64> {
        let state = self.info_client.get_account_state().await?;
        state
            .margin_summary
            .account_value
            .parse::<f64>()
            .map_err(|e| anyhow::anyhow!("Failed to parse account value: {}", e))
    }

    pub async fn get_position(&self, coin: &str) -> Result<Option<super::client::Position>> {
        let state = self.info_client.get_account_state().await?;
        let upper = coin.to_uppercase();
        Ok(state
            .positions
            .into_iter()
            .find(|p| p.position.coin == upper)
            .map(|p| p.position))
    }

    pub async fn cancel_order(&self, oid: u64, coin: &str) -> Result<OrderResult> {
        let coin_normalized = normalize_coin(coin);
        let asset_idx = self.info_client.asset_index(&coin_normalized).await?;

        let action = json!({
            "type": "cancel",
            "cancels": [{
                "a": asset_idx,
                "o": oid
            }]
        });

        self.execute_action(action).await
    }
}
