//! Tiny Shreds Client - Pumpfun Token Detector
//!
//! Connects to shredstream_proxy via gRPC and detects newly minted pumpfun tokens.
//! Uses jito_protos for protocol decoding.

use std::{
    env,
    str::FromStr,
    time::Duration,
};

use jito_protos::shredstream::{
    shredstream_proxy_client::ShredstreamProxyClient,
    SubscribeEntriesRequest,
};
use solana_entry::entry::Entry;
use solana_sdk::pubkey::Pubkey;
use tonic::{metadata::MetadataValue, transport::Endpoint, Request};
use tracing::{info, warn, error};

/// Pumpfun program ID
const PUMPFUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// CREATE instruction discriminator
const CREATE_DISC: [u8; 8] = [24, 30, 200, 40, 5, 28, 7, 119];

/// Process entries and detect pumpfun token creates
fn process_entries(slot: u64, data: &[u8], pumpfun_program_id: &Pubkey) -> usize {
    let entries: Vec<Entry> = match bincode::deserialize(data) {
        Ok(e) => e,
        Err(e) => {
            warn!("Slot {}: Failed to deserialize entries: {}", slot, e);
            return 0;
        }
    };

    let total_txs: usize = entries.iter().map(|e| e.transactions.len()).sum();
    let mut creates_found = 0;

    for entry in &entries {
        for tx in &entry.transactions {
            let accounts = tx.message.static_account_keys();

            for ix in tx.message.instructions() {
                let program_idx = ix.program_id_index as usize;
                if program_idx >= accounts.len() {
                    continue;
                }

                let program_id = &accounts[program_idx];
                if program_id != pumpfun_program_id {
                    continue;
                }

                let data = ix.data.as_slice();
                if data.len() < 8 {
                    continue;
                }

                // Check for CREATE instruction
                if data[0..8] == CREATE_DISC {
                    creates_found += 1;
                    
                    // Extract account pubkeys
                    let ix_accounts: Vec<Pubkey> = ix.accounts
                        .iter()
                        .filter_map(|&idx| accounts.get(idx as usize).copied())
                        .collect();

                    // Account indices for pumpfun CREATE:
                    // 0: mint, 1: mint_authority, 2: bonding_curve, 3: associated_bonding_curve,
                    // 4: global, 5: mpl_token_metadata, 6: metadata, 7: user (creator)
                    
                    let mint = ix_accounts.get(0).map(|p| p.to_string()).unwrap_or_default();
                    let bonding_curve = ix_accounts.get(2).map(|p| p.to_string()).unwrap_or_default();
                    let creator = ix_accounts.get(7).map(|p| p.to_string()).unwrap_or_default();

                    info!("🚀 PUMPFUN TOKEN DETECTED @ Slot {}!", slot);
                    info!("   Mint: {}", mint);
                    info!("   Bonding Curve: {}", bonding_curve);
                    info!("   Creator: {}", creator);
                }
            }
        }
    }

    creates_found
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Configuration
    let shredstream_uri = env::var("SHREDSTREAM_URI")
        .unwrap_or_else(|_| "http://127.0.0.1:9001".to_string());
    let x_token = env::var("X_TOKEN").ok();
    
    let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID)?;

    info!("===========================================");
    info!("  Tiny Shreds Client - Pumpfun Detector");
    info!("===========================================");
    info!("Connecting to: {}", shredstream_uri);
    info!("Pumpfun Program: {}", pumpfun_program_id);
    info!("X-Token: {}", if x_token.is_some() { "set" } else { "not set" });
    info!("");

    loop {
        match connect_and_stream(&shredstream_uri, x_token.as_deref(), &pumpfun_program_id).await {
            Ok(_) => {
                info!("Stream ended. Reconnecting in 1s...");
            }
            Err(e) => {
                error!("Connection error: {}. Reconnecting in 1s...", e);
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn connect_and_stream(
    endpoint: &str,
    x_token: Option<&str>,
    pumpfun_program_id: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = Endpoint::from_str(endpoint)?
        .keep_alive_while_idle(true)
        .http2_keep_alive_interval(Duration::from_secs(5))
        .keep_alive_timeout(Duration::from_secs(10))
        .tcp_keepalive(Some(Duration::from_secs(15)))
        .connect_timeout(Duration::from_secs(5));

    let channel = endpoint.connect().await?;
    let mut client = ShredstreamProxyClient::new(channel);

    let mut request = Request::new(SubscribeEntriesRequest {});
    
    // Add auth token if provided
    if let Some(token) = x_token {
        let metadata_value = MetadataValue::from_str(token)?;
        request.metadata_mut().insert("x-token", metadata_value);
    }

    let mut stream = client.subscribe_entries(request).await?.into_inner();
    
    info!("✅ Connected to shredstream!");
    info!("Waiting for entries...");
    info!("");

    let mut entry_count = 0u64;
    let mut bytes_total = 0u64;
    let mut creates_total = 0usize;
    let mut last_stats = std::time::Instant::now();

    while let Some(result) = stream.message().await.transpose() {
        match result {
            Ok(entry) => {
                entry_count += 1;
                let slot = entry.slot;
                let data = entry.entries;
                bytes_total += data.len() as u64;

                if entry_count == 1 {
                    info!("🎉 First entry! Slot: {}, Size: {} bytes", slot, data.len());
                }

                // Process entries and look for pumpfun creates
                let creates = process_entries(slot, &data, pumpfun_program_id);
                creates_total += creates;

                // Log stats every 15 seconds
                if last_stats.elapsed() >= Duration::from_secs(15) {
                    info!(
                        "📊 Stats: {} entries, {:.2} MB, {} pumpfun creates",
                        entry_count,
                        bytes_total as f64 / 1_000_000.0,
                        creates_total
                    );
                    entry_count = 0;
                    bytes_total = 0;
                    creates_total = 0;
                    last_stats = std::time::Instant::now();
                }
            }
            Err(e) => {
                error!("Stream error: {:?}", e);
                return Err(Box::new(e));
            }
        }
    }

    Ok(())
}
