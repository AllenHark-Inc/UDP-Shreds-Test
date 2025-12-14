//! Tiny Shreds UDP Client - Pumpfun Token Detector
//!
//! Listens for UDP packets from shredstream_proxy and detects newly minted pumpfun tokens.

use std::{
    collections::HashMap,
    str::FromStr,
    time::{Duration, Instant},
};

use solana_entry::entry::Entry;
use solana_sdk::pubkey::Pubkey;
use tokio::net::UdpSocket;
use tracing::{info, warn, error, debug};

/// Pumpfun program ID
const PUMPFUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// CREATE instruction discriminator
const CREATE_DISC: [u8; 8] = [24, 30, 200, 40, 5, 28, 7, 119];

/// Fragment header size
const HEADER_SIZE: usize = 16;

/// Magic bytes for fragmented messages
const MAGIC: &[u8; 4] = b"SHRD";

/// Fragment reassembler for handling multi-packet messages
struct FragmentReassembler {
    buffers: HashMap<u32, FragmentBuffer>,
}

struct FragmentBuffer {
    total_fragments: u16,
    total_size: u32,
    received: HashMap<u16, Vec<u8>>,
    created_at: Instant,
}

impl FragmentReassembler {
    fn new() -> Self {
        Self { buffers: HashMap::new() }
    }

    /// Process incoming packet, returns complete message if reassembly is done
    fn process_packet(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        // Check if this is a fragmented message (starts with SHRD magic)
        if data.len() >= HEADER_SIZE && &data[0..4] == MAGIC {
            let message_id = u32::from_le_bytes(data[4..8].try_into().unwrap());
            let fragment_index = u16::from_le_bytes(data[8..10].try_into().unwrap());
            let total_fragments = u16::from_le_bytes(data[10..12].try_into().unwrap());
            let total_size = u32::from_le_bytes(data[12..16].try_into().unwrap());
            let fragment_data = data[HEADER_SIZE..].to_vec();

            debug!(
                "Fragment: msg_id={}, idx={}/{}, size={}",
                message_id, fragment_index + 1, total_fragments, fragment_data.len()
            );

            let entry = self.buffers.entry(message_id).or_insert_with(|| FragmentBuffer {
                total_fragments,
                total_size,
                received: HashMap::new(),
                created_at: Instant::now(),
            });

            entry.received.insert(fragment_index, fragment_data);

            // Check if complete
            if entry.received.len() == total_fragments as usize {
                let mut complete = Vec::with_capacity(total_size as usize);
                for i in 0..total_fragments {
                    if let Some(frag) = entry.received.get(&i) {
                        complete.extend_from_slice(frag);
                    }
                }
                self.buffers.remove(&message_id);
                info!("Reassembled message: {} bytes from {} fragments", complete.len(), total_fragments);
                return Some(complete);
            }
            None
        } else {
            // Non-fragmented message - return as-is
            Some(data.to_vec())
        }
    }

    /// Cleanup old incomplete buffers (call periodically)
    fn cleanup_old(&mut self) {
        let max_age = Duration::from_secs(10);
        self.buffers.retain(|_, v| v.created_at.elapsed() < max_age);
    }
}

/// Process entries and detect pumpfun token creates
fn process_entries(data: &[u8], pumpfun_program_id: &Pubkey) -> usize {
    let entries: Vec<Entry> = match bincode::deserialize(data) {
        Ok(e) => e,
        Err(e) => {
            warn!("Failed to deserialize entries: {}", e);
            return 0;
        }
    };

    let total_txs: usize = entries.iter().map(|e| e.transactions.len()).sum();
    debug!("Processing {} entries with {} transactions", entries.len(), total_txs);
    
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
                    
                    let ix_accounts: Vec<Pubkey> = ix.accounts
                        .iter()
                        .filter_map(|&idx| accounts.get(idx as usize).copied())
                        .collect();

                    // 0: mint, 2: bonding_curve, 7: creator
                    let mint = ix_accounts.get(0).map(|p| p.to_string()).unwrap_or_default();
                    let bonding_curve = ix_accounts.get(2).map(|p| p.to_string()).unwrap_or_default();
                    let creator = ix_accounts.get(7).map(|p| p.to_string()).unwrap_or_default();

                    info!("🚀 PUMPFUN TOKEN DETECTED!");
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
    tracing_subscriber::fmt::init();

    let bind_addr = std::env::var("UDP_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:9001".to_string());
    let pumpfun_program_id = Pubkey::from_str(PUMPFUN_PROGRAM_ID)?;

    info!("===========================================");
    info!("  Tiny Shreds UDP Client - Pumpfun Detector");
    info!("===========================================");
    info!("Listening on: {}", bind_addr);
    info!("Pumpfun Program: {}", pumpfun_program_id);
    info!("");

    let socket = UdpSocket::bind(&bind_addr).await?;
    info!("✅ UDP socket bound successfully!");
    info!("Waiting for packets from shredstream_proxy...");
    info!("");

    let mut reassembler = FragmentReassembler::new();
    let mut buf = vec![0u8; 65536];
    
    let mut packets_received = 0u64;
    let mut bytes_received = 0u64;
    let mut creates_total = 0usize;
    let mut last_stats = Instant::now();
    let mut last_cleanup = Instant::now();

    loop {
        let (len, src) = socket.recv_from(&mut buf).await?;
        packets_received += 1;
        bytes_received += len as u64;

        if packets_received == 1 {
            info!("🎉 First packet from {}! ({} bytes)", src, len);
        }

        // Cleanup old fragments every 5 seconds
        if last_cleanup.elapsed() >= Duration::from_secs(5) {
            reassembler.cleanup_old();
            last_cleanup = Instant::now();
        }

        // Process packet through reassembler
        if let Some(complete_data) = reassembler.process_packet(&buf[..len]) {
            creates_total += process_entries(&complete_data, &pumpfun_program_id);
        }

        // Log stats every 15 seconds
        if last_stats.elapsed() >= Duration::from_secs(15) {
            info!(
                "📊 {} packets, {:.2} MB, {} pumpfun creates",
                packets_received,
                bytes_received as f64 / 1_000_000.0,
                creates_total
            );
            packets_received = 0;
            bytes_received = 0;
            creates_total = 0;
            last_stats = Instant::now();
        }
    }
}
