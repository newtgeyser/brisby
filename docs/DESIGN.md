# Brisby: Privacy-Preserving P2P File Sharing

## Design Document

**Status:** Draft
**Last Updated:** 2025-12-15

---

## Table of Contents

1. [Introduction](#1-introduction)
2. [Goals and Non-Goals](#2-goals-and-non-goals)
3. [Threat Model](#3-threat-model)
4. [System Overview](#4-system-overview)
5. [Architecture](#5-architecture)
   - 5.1 Node Types
   - 5.2 Network Topology
   - 5.3 Protocol Stack
6. [Core Components](#6-core-components)
   - 6.1 Nym Integration
   - 6.2 File Management
   - 6.3 Search System
   - 6.4 DHT (Peer Discovery)
   - 6.5 Transfer Protocol
7. [Data Structures](#7-data-structures)
8. [Wire Protocol](#8-wire-protocol)
9. [Security Considerations](#9-security-considerations)
10. [Technology Choices](#10-technology-choices)
11. [Implementation Phases](#11-implementation-phases)
12. [Open Questions](#12-open-questions)
13. [Appendix](#13-appendix)

---

## 1. Introduction

### 1.1 Purpose

Brisby is a privacy-preserving peer-to-peer file sharing system optimized for files ranging from 10MB to a few GB. It enables users to search for and share files without revealing their identity or network location.

### 1.2 Background

Existing file sharing solutions either lack privacy (BitTorrent exposes IP addresses in swarms) or have usability issues (I2P's complexity, Freenet's performance). Brisby aims to provide strong privacy guarantees while maintaining reasonable performance and a simple user experience.

### 1.3 Scope

This document covers the technical design of Brisby, including architecture, protocols, data structures, and implementation approach.

---

## 2. Goals and Non-Goals

### 2.1 Goals

- **Privacy**: Protect the identity of both file sharers and downloaders
- **Searchability**: Full-text search across file names and metadata
- **Performance**: Reasonable transfer speeds for GB-scale files via parallel chunking
- **Simplicity**: Single privacy layer (Nym), minimal external dependencies
- **Decentralization**: No single point of failure for core functionality

### 2.2 Non-Goals

- **Content privacy**: The system protects *who* shares and downloads, not *what* is shared. File contents and metadata (filenames, keywords) are visible to index providers and can be discovered by anyone searching. Users wanting content privacy should encrypt files before sharing.
- **Real-time streaming**: Optimized for file downloads, not streaming media
- **Guaranteed availability**: No redundant storage; files available only when seeders are online
- **Seeding incentives**: Initially altruistic; future phases may explore incentive mechanisms

---

## 3. Threat Model

### 3.1 Adversary Capabilities

| Adversary Type | Capabilities | In Scope? |
|----------------|--------------|-----------|
| Global passive | Observes all network traffic | Yes |
| Local active | Controls some nodes, can inject traffic | Yes |
| Index provider | Sees all search queries (not origins) | Yes |
| Malicious peers | Serve bad data, refuse to serve | Yes |
| State-level | Compel index providers, block Nym | Partial |

### 3.2 What We Protect

- **Query unlinkability**: Cannot link a search query to its originator's IP
- **Download unlinkability**: Cannot link a file download to the downloader's IP
- **Seeder privacy**: Cannot identify who is sharing a specific file
- **Social graph**: Cannot determine who communicates with whom

### 3.3 What We Do Not Protect

- **Query content**: Index providers see search terms (but not who searched)
- **File existence**: Published files are publicly discoverable
- **Timing attacks**: Partial protection via Nym mixing, not guaranteed
- **Application-level leaks**: Out of scope (user responsibility)

### 3.4 Trust Assumptions

- Nym mixnet provides unlinkability (honest majority of mix nodes)
- At least one index provider is honest (for search availability)
- DHT has sufficient honest nodes (address churn limits Sybil attacks)

---

## 4. System Overview

### 4.1 High-Level Architecture

```
[User Nodes] <--[Nym Mixnet]--> [Index Providers]
                    |
                    +--> [DHT Nodes]
                    |
                    +--> [Other User Nodes]
```

### 4.2 Key Workflows

1. **Publishing**: User announces file metadata to index providers and DHT
2. **Searching**: User queries index providers, receives matching files
3. **Discovery**: User queries DHT to find peers with desired file
4. **Transfer**: User downloads chunks from peers via Nym

### 4.3 Design Principles

- All inter-node communication goes through Nym
- Federated index providers for search (no single point of failure)
- Lightweight DHT for peer discovery (exact key lookup only)
- Parallel chunk transfers to overcome mixnet latency
- Nym bandwidth credentials handle network-level payment

---

## 5. Architecture

### 5.1 Node Types

#### 5.1.1 User Node

The standard client that all users run.

**Responsibilities:**
- Manage local files (chunking, hashing, storage)
- Maintain local search index of own files
- Query index providers for search
- Participate in DHT (lightweight)
- Download/upload file chunks

**Components:**
- Nym client (embedded, handles bandwidth credentials)
- File manager
- Local index (SQLite FTS5)
- DHT client
- Transfer manager
- CLI/GUI

#### 5.1.2 Index Provider

Federated search servers that provide full-text search.

**Responsibilities:**
- Index file metadata from publishers
- Handle search queries
- Replicate index with peer providers
- Optionally run full Nym node (earn from mixing)

**Components:**
- Nym node (full)
- Search engine (SQLite FTS5)
- API handler
- Replication manager

#### 5.1.3 DHT Node

Distributed hash table nodes for peer discovery.

**Responsibilities:**
- Store content_hash → seeder mappings
- Route DHT lookups
- Can be colocated with user node

**Components:**
- Nym client
- Kademlia routing table
- Key-value storage

### 5.2 Network Topology

```
                    ┌─────────────────┐
                    │  Index Provider │
                    │    Cluster      │
                    └────────┬────────┘
                             │
                        [Nym Mixnet]
                             │
    ┌────────────────────────┼────────────────────────┐
    │                        │                        │
┌───┴───┐              ┌─────┴─────┐              ┌───┴───┐
│ User  │◄────────────►│    DHT    │◄────────────►│ User  │
│ Node  │   [Nym]      │  Overlay  │    [Nym]     │ Node  │
└───────┘              └───────────┘              └───────┘
```

### 5.3 Protocol Stack

| Layer | Purpose | Implementation |
|-------|---------|----------------|
| Application | Search, transfer | Brisby protocol |
| Message | Request/response framing | Protobuf |
| Routing | DHT lookups, index provider selection | Kademlia |
| Privacy | Anonymity, unlinkability | Nym mixnet |
| Transport | Network connectivity | TCP/WebSocket |

---

## 6. Core Components

### 6.1 Nym Integration

#### 6.1.1 Client Mode (User Nodes)

- Embed nym-sdk Rust client
- Maintain persistent Nym address for receiving responses
- Use SURBs (Single-Use Reply Blocks) for anonymous responses
- Handle connection management and reconnection

#### 6.1.2 Node Mode (Index Providers)

- Run full Nym node to earn from mixing traffic
- Expose service endpoint through Nym
- Handle high request volume

#### 6.1.3 Message Handling

- All messages wrapped in Nym Sphinx packets
- Responses use SURB when provided
- Timeout and retry logic for unreliable delivery

### 6.2 File Management

#### 6.2.1 Chunking Strategy

- Fixed chunk size: 256 KB
- Content-addressed: chunk_hash = BLAKE3(chunk_data)
- File hash: BLAKE3(concatenated chunk hashes)

#### 6.2.2 Local Storage

```
~/.brisby/
├── config.toml
├── index.db           # SQLite FTS5 for local files
├── chunks/            # Stored chunks (by hash)
│   ├── ab/
│   │   └── ab3f...    # First 2 chars as directory
│   └── ...
├── downloads/         # In-progress downloads
└── nym/               # Nym client data (keys, credentials)
```

#### 6.2.3 Metadata Extraction

- Filename parsing (keywords)
- MIME type detection
- Optional: user-provided tags

### 6.3 Search System

#### 6.3.1 Index Provider Selection

- Maintain list of known providers (bootstrapped + discovered)
- Query multiple providers in parallel
- Merge and deduplicate results by content_hash

#### 6.3.2 Query Processing (at Provider)

- Full-text search using SQLite FTS5
- Fields: filename, keywords
- Ranking by relevance score
- Result limit (default 50)

#### 6.3.3 Publishing

- User publishes metadata to multiple index providers
- Providers replicate among themselves
- TTL-based expiration (re-announce to stay listed)

### 6.4 DHT (Peer Discovery)

#### 6.4.1 Kademlia Adaptation

- Standard Kademlia with XOR distance metric
- All messages routed through Nym (adds latency)
- Reduced lookup parallelism (α=3) due to latency
- Larger K-buckets (K=20) for resilience

#### 6.4.2 Stored Data

- Key: content_hash (32 bytes)
- Value: list of seeders (nym_address, chunk_bitmap, last_seen)

#### 6.4.3 Operations

- **FIND_NODE**: Locate nodes close to a key
- **FIND_VALUE**: Find seeders for content_hash
- **STORE**: Announce availability of a file
- **PING**: Liveness check

#### 6.4.4 Bootstrap

- Hardcoded bootstrap nodes initially
- Learn more nodes through DHT operation
- Persist known nodes across sessions

### 6.5 Transfer Protocol

#### 6.5.1 Chunk Request/Response

- Request includes: content_hash, chunk_index, SURB
- Response includes: chunk_data, chunk_hash
- Verify hash before accepting
- Nym bandwidth credentials handle payment at network layer

#### 6.5.2 Parallel Downloads

- Concurrent requests: 50-100 (configurable)
- Semaphore-based rate limiting
- Round-robin or random seeder selection
- Retry failed chunks with different seeder

#### 6.5.3 Flow Control

- Track in-flight requests
- Timeout: 30 seconds (configurable)
- Exponential backoff on retry
- Seeder reputation tracking (prefer reliable peers)

---

## 7. Data Structures

### 7.1 File Metadata

```
FileMetadata {
    content_hash: [u8; 32]      // BLAKE3 of file
    filename: String
    size: u64
    mime_type: Option<String>
    chunks: Vec<ChunkInfo>
    keywords: Vec<String>
    created_at: u64             // Unix timestamp
}

ChunkInfo {
    index: u32
    hash: [u8; 32]              // BLAKE3 of chunk
    size: u32                   // Actual size (last may differ)
}
```

### 7.2 Index Entry (at Index Providers)

```
IndexEntry {
    content_hash: [u8; 32]
    filename: String            // Searchable
    keywords: Vec<String>       // Searchable
    size: u64
    chunk_count: u32
    published_at: u64
    ttl: u64                    // Expiration
    publisher_reputation: f32
}
```

### 7.3 DHT Entry

```
DhtKey: [u8; 32]                // content_hash

DhtValue {
    seeders: Vec<Seeder>
}

Seeder {
    nym_address: NymAddress
    chunk_bitmap: BitVec        // Which chunks available
    last_seen: u64
}
```

---

## 8. Wire Protocol

### 8.1 Protocol Version

- Current version: **1**
- Version included in every message envelope
- Nodes should reject messages with incompatible major versions
- Minor version bumps for backward-compatible additions

### 8.2 Message Format

All messages use Protocol Buffers, wrapped in Nym Sphinx packets.

```
Envelope {
    version: u8               // Protocol version
    request_id: u64           // For request/response correlation
    payload: oneof { ... }    // Message-specific content
}
```

### 8.3 Message Types

#### Search
- SearchRequest { query, max_results }
- SearchResponse { results[] }

#### Transfer
- ChunkRequest { content_hash, chunk_index, surb }
- ChunkResponse { content_hash, chunk_index, data, chunk_hash }

#### Publishing
- PublishRequest { content_hash, filename, keywords, size, chunk_count, nym_address }
- PublishResponse { success, error? }

#### DHT
- FindNodeRequest { target_id }
- FindNodeResponse { nodes[] }
- FindValueRequest { key }
- FindValueResponse { seeders[] | nodes[] }
- StoreRequest { key, seeder_info }
- StoreResponse { success }

#### Errors
- ErrorResponse { code, message }

### 8.4 Error Handling

- Timeout: 30s default, retry with backoff
- Chunk hash mismatch: reject, try different seeder
- Unknown message: ignore (forward compatibility)
- Seeder unavailable: remove from local cache, try next
- Version mismatch: respond with ErrorResponse indicating supported version

---

## 9. Security Considerations

### 9.1 Threat Mitigations

| Threat | Mitigation |
|--------|------------|
| IP correlation | All traffic through Nym mixnet |
| Malicious index provider | Query multiple, compare results |
| Chunk poisoning | Verify BLAKE3 hash before accepting |
| Sybil attack on DHT | Nym address churn, reputation tracking (future) |
| Replay attacks | Request IDs, SURB single-use property |
| Timing correlation | Nym mixing provides cover traffic |
| Freeloading | Altruistic model; future phases may add incentives |

### 9.2 Cryptographic Choices

| Purpose | Algorithm |
|---------|-----------|
| Hashing | BLAKE3 |
| Signatures | Ed25519 (for Nym identity) |
| Key exchange | X25519 (Nym internal) |

Note: Nym handles most cryptography internally. We primarily use BLAKE3 for content addressing.

### 9.3 Known Limitations

- Query content visible to index providers
- Active adversary controlling many mix nodes can correlate
- No seeding incentives (altruistic model)
- File content not encrypted (user responsibility)
- Requires NYM tokens for network access

---

## 10. Technology Choices

### 10.1 Language

**Rust** - Memory safety, performance, excellent async support, Nym SDK is Rust-native.

### 10.2 Dependencies

| Component | Library |
|-----------|---------|
| Async runtime | tokio |
| Nym integration | nym-sdk |
| Serialization | prost (protobuf) |
| Database & search | rusqlite + FTS5 |
| Hashing | blake3 |
| CLI | clap |
| GUI (future) | tauri |

### 10.3 Build and Distribution

- Single binary distribution
- Cross-platform: Linux, macOS, Windows
- Optional: Docker image for index providers

---

## 11. Implementation Phases

### Phase 1: Minimal Viable Product

**Goal:** End-to-end file sharing works, single index provider.

- [ ] Project scaffolding (workspace, crates)
- [ ] Nym client integration (connect, send, receive, bandwidth credentials)
- [ ] File chunking and reassembly
- [ ] Basic protobuf messages
- [ ] Hardcoded single index provider
- [ ] Simple filename search (SQLite FTS5, exact match queries)
- [ ] Sequential chunk download
- [ ] Basic CLI (share, search, download)

**Exit criteria:** Can share a file on one machine and download on another via Nym.

### Phase 2: Usable System

**Goal:** Multiple providers, full-text search, parallel downloads.

- [ ] Parallel chunk downloads (50+ concurrent)
- [ ] Full-text search (SQLite FTS5 with ranking)
- [ ] Multiple index providers (query, merge)
- [ ] DHT implementation (Kademlia over Nym)
- [ ] Seeder selection and failover
- [ ] Improved CLI UX
- [ ] Download resume support

**Exit criteria:** Usable for real file sharing with reasonable performance.

### Phase 3: Production Ready

**Goal:** Hardened DHT, GUI, operational readiness.

- [ ] DHT Sybil resistance (reputation, rate limiting)
- [ ] Index provider replication protocol
- [ ] GUI application (Tauri)
- [ ] Seeding incentives exploration (optional, based on usage patterns)
- [ ] Documentation and deployment guides
- [ ] Index provider operational tooling

**Exit criteria:** Ready for public beta.

---

## 12. Open Questions

### 12.1 Architecture

- **Q:** Should DHT nodes be separate from user nodes, or always colocated?
- **Q:** How many index providers is "enough" for resilience?
- **Q:** Should we support file encryption at the protocol level?

### 12.2 Performance

- **Q:** What's the realistic throughput over Nym for large files?
- **Q:** How does DHT lookup latency scale with network size?

### 12.3 Operations

- **Q:** How to bootstrap the network (chicken-and-egg)?
- **Q:** Who runs index providers initially?
- **Q:** How to handle index provider discovery?

### 12.4 Future Incentives

- **Q:** How to incentivize seeding if altruistic model proves insufficient?
- **Q:** Could heavy leechers subsidize bandwidth for seeders?
- **Q:** Integration with Nym's credential system for seeder rewards?

---

## 13. Appendix

### 13.1 Glossary

| Term | Definition |
|------|------------|
| Chunk | Fixed-size piece of a file (256 KB) |
| Content hash | BLAKE3 hash identifying a file |
| DHT | Distributed Hash Table for peer discovery |
| Index provider | Server providing full-text search |
| Nym | Mixnet providing network-level privacy |
| SURB | Single-Use Reply Block for anonymous responses |
| Seeder | Peer sharing a file |
| Leecher | Peer downloading a file |

### 13.2 References

- [Nym Documentation](https://nym.com/docs/)
- [Nym SDK](https://sdk.nymtech.net/)
- [Kademlia Paper](https://pdos.csail.mit.edu/~petar/papers/maymounkov-kademlia-lncs.pdf)
- [BitTorrent Protocol](https://www.bittorrent.org/beps/bep_0003.html)
- [BLAKE3](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf)

### 13.3 Revision History

| Date | Author | Changes |
|------|--------|---------|
| 2025-12-15 | - | Initial draft |
