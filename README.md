# Brisby

Privacy-preserving peer-to-peer file sharing over the [Nym](https://nymtech.net/) mixnet.

Brisby enables anonymous file sharing by routing all traffic through Nym's mixnet, providing strong metadata privacy. Seeders and downloaders cannot be linked, and index providers cannot identify who is searching or sharing files.

## Features

- **Privacy by default** - All communication routed through Nym mixnet
- **Decentralized search** - Index providers store searchable file metadata
- **Content-addressed** - Files identified by BLAKE3 hash
- **Chunked transfers** - Large files split into 256KB chunks
- **Resumable downloads** - Download chunks from multiple seeders
- **Full-text search** - SQLite FTS5-powered search index

## Architecture

```
┌─────────────┐         ┌─────────────┐         ┌─────────────┐
│   Client    │◄──Nym──►│   Index     │◄──Nym──►│   Seeder    │
│  (search/   │         │  Provider   │         │  (serves    │
│   download) │         │  (search)   │         │   chunks)   │
└─────────────┘         └─────────────┘         └─────────────┘
       │                                               │
       └──────────────────Nym─────────────────────────►┘
                    (chunk requests)
```

### Components

| Crate | Description |
|-------|-------------|
| `brisby-core` | Shared types, protocol messages, transport abstraction |
| `brisby-client` | CLI client for searching, downloading, and seeding |
| `brisby-index` | Index provider service for searchable file metadata |
| `brisby-dht` | Distributed hash table for peer discovery (WIP) |

## Building

### Prerequisites

- Rust 1.70+ (2021 edition)
- SQLite development libraries

### Build

```bash
# Build all crates (without Nym, for development)
cargo build --release

# Build with Nym support (required for real network usage)
cargo build --release --features nym

# Build specific binaries
cargo build --release -p brisby-client --features nym
cargo build --release -p brisby-index --features nym
```

The first build with Nym support takes ~20-30 minutes due to Nym SDK compilation.

## Usage

### Initialize Configuration

```bash
brisby init
```

Creates `~/.brisby/` with default configuration and directories.

### Sharing Files (Seeding)

Start seeding files to make them available on the network:

```bash
# Seed a single file
brisby --index-provider <INDEX_ADDR> seed -f myfile.txt

# Seed and publish to index provider (makes file searchable)
brisby --index-provider <INDEX_ADDR> seed -f myfile.txt -p

# Seed multiple files
brisby --index-provider <INDEX_ADDR> seed -f file1.txt -f file2.pdf -p

# Seed all previously added files
brisby seed
```

The seeder will:
1. Chunk the file and compute content hash
2. Connect to Nym network and display its address
3. Optionally publish metadata to the index provider
4. Listen for chunk requests from other peers

### Searching for Files

```bash
# Search for files
brisby --index-provider <INDEX_ADDR> search "query"

# Limit results
brisby --index-provider <INDEX_ADDR> search "movie" --max-results 10
```

Search results include:
- Filename and size
- Content hash (for downloading)
- Number of chunks
- Number of known seeders

### Downloading Files

```bash
# Download a file by hash
brisby download <CONTENT_HASH> -s <SEEDER_ADDR> -c <CHUNK_COUNT>

# Specify output path
brisby download <HASH> -s <SEEDER> -c 4 -o downloaded.txt

# Download from multiple seeders
brisby download <HASH> -s <SEEDER1> -s <SEEDER2> -c 10
```

### Running an Index Provider

Index providers maintain a searchable database of file metadata:

```bash
# Start index provider
brisby-index -d /path/to/data

# With verbose logging
brisby-index -d /path/to/data -v
```

The index provider will display its Nym address on startup. Share this address with users who want to search your index.

### Global Options

```bash
brisby [OPTIONS] <COMMAND>

Options:
  -c, --config <FILE>       Config file [default: ~/.brisby/config.toml]
  -d, --data-dir <DIR>      Data directory [default: ~/.brisby]
  -v, --verbose             Enable verbose output
  --index-provider <ADDR>   Index provider Nym address
  --mock                    Use mock transport (testing only)
```

### Example Session

Terminal 1 - Start index provider:
```bash
./target/release/brisby-index -d /tmp/index
# Output: Address: ABC123...
```

Terminal 2 - Start seeding a file:
```bash
./target/release/brisby --index-provider ABC123... seed -f document.pdf -p
# Output: Address: DEF456...
# Output: Published: document.pdf
```

Terminal 3 - Search and download:
```bash
# Search
./target/release/brisby --index-provider ABC123... search "document"
# Output: 1. document.pdf (1234567 bytes, 5 chunks)
#         Hash: 789ABC...
#         Seeders: 1

# Download
./target/release/brisby download 789ABC... -s DEF456... -c 5 -o document.pdf
```

## Testing

### Quick Tests (No Network)

```bash
# Run all unit tests
cargo test

# Run client tests including integration tests
cargo test -p brisby-client

# Run specific test
cargo test -p brisby-client test_full_flow_mock
```

### Integration Tests

The integration test script tests the full flow with real or mock Nym:

```bash
# Quick mock test (~5 seconds, no network)
./scripts/integration-test.sh --mock

# Full Nym network test (~3-5 minutes)
./scripts/integration-test.sh
```

The script:
1. Starts an index provider
2. Starts a seeder with a test file
3. Publishes to the index
4. Searches for the file
5. Downloads and verifies content

### Manual Testing

For development without Nym network delays:

```bash
# Use mock transport
brisby --mock seed -f test.txt
brisby --mock --index-provider mock-addr search "test"
```

## Configuration

Configuration file: `~/.brisby/config.toml`

```toml
[network]
# Default index provider address
index_provider = ""

[storage]
# Chunk storage directory
chunks_dir = "~/.brisby/chunks"
# Download directory
downloads_dir = "~/.brisby/downloads"
```

## Protocol

Brisby uses a custom binary protocol over Nym:

| Message | Description |
|---------|-------------|
| `SearchRequest` | Query index provider for files |
| `SearchResponse` | List of matching files with seeder info |
| `PublishRequest` | Register file metadata with index |
| `PublishResponse` | Confirmation of registration |
| `ChunkRequest` | Request specific chunk from seeder |
| `ChunkResponse` | Chunk data with verification hash |

All messages are encoded with [prost](https://github.com/tokio-rs/prost) (Protocol Buffers).

## Privacy Considerations

- **Metadata protection**: Nym mixnet hides IP addresses and timing
- **Unlinkability**: Index providers can't link searches to downloads
- **No central authority**: Anyone can run an index provider
- **Content hashing**: Files identified by content, not names

**Limitations**:
- File content is not encrypted (use encrypted archives if needed)
- Index providers see search queries (but not who searched)
- Seeders see chunk requests (but not full download context)

## Project Status

This is an early-stage project. Current status:

- [x] Core protocol and types
- [x] File chunking with BLAKE3
- [x] Mock transport for testing
- [x] Nym transport integration
- [x] Index provider with FTS search
- [x] Client search functionality
- [x] Client download functionality
- [x] Seeder functionality
- [ ] DHT for decentralized peer discovery
- [ ] Chunk caching and deduplication
- [ ] Bandwidth management
- [ ] GUI client

## License

MIT License - see [LICENSE](LICENSE) for details.

## Acknowledgments

- [Nym](https://nymtech.net/) for the mixnet infrastructure
- [BLAKE3](https://github.com/BLAKE3-team/BLAKE3) for fast, secure hashing
- [SQLite](https://sqlite.org/) with FTS5 for full-text search
