# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-02-03

### Added
- `GET /follows?pubkey=xxx` - Returns array of pubkeys that the given pubkey follows
- `GET /common-follows?from=xxx&to=yyy` - Returns array of pubkeys that both from and to follow (mutual follows)
- `GET /path?from=xxx&to=yyy` - Returns array of pubkeys forming the shortest path between two pubkeys

## [0.1.0] - Initial Release

### Added
- Core Web of Trust graph indexing from Nostr relays
- `GET /health` - Health check endpoint
- `GET /stats` - Graph and cache statistics
- `GET /distance` - Query social distance between two pubkeys
- `POST /distance/batch` - Batch distance queries (up to 100 targets)
- Bidirectional BFS algorithm for efficient path finding
- LRU cache with TTL for query results
- Per-IP rate limiting
- Optional DVM (NIP-90) interface
- SQLite persistence for graph state
