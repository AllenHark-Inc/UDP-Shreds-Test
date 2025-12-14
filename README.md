# Shreds UDP Client - Pumpfun Token Detector

A lightweight Rust UDP client that listens for Solana shred data and detects newly minted Pumpfun tokens in real-time.

This is an example implementation that can be compiled and customized for your own use case.

**Links:**
- 🌐 Website: [allenhark.com](https://allenhark.com)
- 💬 Discord: [Join our community](https://discord.gg/JpzS72MAKG)

## Features

- **UDP Listener** - Receives shred data on a configurable port
- **Fragment Reassembly** - Handles large messages split across multiple UDP packets
- **Pumpfun Detection** - Scans transactions for Pumpfun CREATE instructions
- **Real-time Logging** - Prints token details immediately when detected

## Requirements

- Rust 1.70+
- A shred data source sending bincode-serialized entries via UDP

## Build

```bash
cargo build --release
```

## Run

```bash
# Default port 9001
./target/release/test_shreds

# Custom port
UDP_BIND_ADDR=0.0.0.0:8888 ./target/release/test_shreds
```

## Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `UDP_BIND_ADDR` | `0.0.0.0:9001` | Address and port to listen on |
| `RUST_LOG` | `info` | Log level (debug, info, warn, error) |

## Output

When running, you'll see:

```
📦 Msg #123: 45 entries, 892 txs
```

When a Pumpfun token is detected:

```
═══════════════════════════════════════════════════════
🚀 PUMPFUN TOKEN FOUND!
   Token Address: 7xKX...
   Bonding Curve: 9yLM...
   Creator: 3zAB...
   Message: #123, Entries: 45, Txs: 892
═══════════════════════════════════════════════════════
```

## Data Format

The client expects UDP packets containing:

1. **Single packets**: Raw bincode-serialized `Vec<solana_entry::entry::Entry>`
2. **Fragmented packets**: 16-byte header (`SHRD` magic + metadata) followed by payload chunk

Fragment header format:
- Bytes 0-3: Magic `SHRD`
- Bytes 4-7: Message ID (u32 LE)
- Bytes 8-9: Fragment index (u16 LE)
- Bytes 10-11: Total fragments (u16 LE)
- Bytes 12-15: Total message size (u32 LE)

## Extending

To add detection for other programs or instructions, modify `process_entries()` in `src/main.rs`:

```rust
// Add your instruction discriminator
const MY_INSTRUCTION_DISC: [u8; 8] = [...];

// Check for it in the instruction loop
if data[0..8] == MY_INSTRUCTION_DISC {
    // Handle your instruction
}
```

## License

MIT
