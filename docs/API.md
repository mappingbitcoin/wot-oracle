# API Reference

WoT Oracle exposes a REST API for querying social distance in the Nostr follow graph.

**Base URL:** `http://localhost:8080` (configurable via `HTTP_PORT`)

## Endpoints

### GET /health

Health check endpoint.

**Response:**
```json
{
  "status": "healthy",
  "version": "0.1.0"
}
```

---

### GET /stats

Returns graph statistics and cache metrics.

**Response:**
```json
{
  "node_count": 150000,
  "edge_count": 2500000,
  "nodes_with_follows": 120000,
  "cache": {
    "size": 5432,
    "hits": 12345,
    "misses": 678
  },
  "locks": {
    "read_count": 100000,
    "write_count": 5000,
    "read_wait_ns": 123456,
    "write_wait_ns": 78901
  }
}
```

---

### GET /distance

Query the social distance between two pubkeys.

**Parameters:**

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `from` | string | Yes | - | Source pubkey (64 hex chars) |
| `to` | string | Yes | - | Target pubkey (64 hex chars) |
| `max_hops` | integer | No | 5 | Maximum hops to search (1-10) |
| `include_bridges` | boolean | No | false | Include bridge node pubkeys |
| `bypass_cache` | boolean | No | false | Skip cache, force fresh computation |

**Example:**
```bash
curl "http://localhost:8080/distance?from=82341f...&to=3bf0c6...&include_bridges=true"
```

**Response:**
```json
{
  "from": "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
  "to": "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
  "hops": 2,
  "path_count": 3,
  "mutual_follow": false,
  "bridges": [
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52"
  ]
}
```

**Response Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `from` | string | Source pubkey |
| `to` | string | Target pubkey |
| `hops` | integer or null | Number of hops (null if not reachable) |
| `path_count` | integer | Number of shortest paths found |
| `mutual_follow` | boolean | Whether from and to follow each other |
| `bridges` | array or null | Pubkeys where paths meet (if `include_bridges=true`) |

**Error Response:**
```json
{
  "error": "Invalid pubkey length: expected 64, got 32",
  "code": "INVALID_PUBKEY_LENGTH"
}
```

**Error Codes:**
- `INVALID_PUBKEY_LENGTH` - Pubkey must be 64 characters
- `INVALID_PUBKEY_FORMAT` - Pubkey must be hexadecimal
- `INVALID_MAX_HOPS` - max_hops must be 1-10
- `INTERNAL_ERROR` - Server error

---

### POST /distance/batch

Query distances from one pubkey to multiple targets in a single request.

**Request Body:**
```json
{
  "from": "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
  "targets": [
    "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52"
  ],
  "max_hops": 5,
  "include_bridges": false,
  "bypass_cache": false
}
```

**Parameters:**

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `from` | string | Yes | - | Source pubkey (64 hex chars) |
| `targets` | array | Yes | - | Target pubkeys (max 100) |
| `max_hops` | integer | No | 5 | Maximum hops to search (1-10) |
| `include_bridges` | boolean | No | false | Include bridge node pubkeys |
| `bypass_cache` | boolean | No | false | Skip cache, force fresh computation |

**Example:**
```bash
curl -X POST http://localhost:8080/distance/batch \
  -H "Content-Type: application/json" \
  -d '{
    "from": "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
    "targets": ["3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"]
  }'
```

**Response:**
```json
{
  "from": "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
  "results": [
    {
      "from": "82341f...",
      "to": "3bf0c6...",
      "hops": 2,
      "path_count": 1,
      "mutual_follow": false
    }
  ]
}
```

**Error Codes:**
- `TOO_MANY_TARGETS` - Maximum 100 targets per batch

---

## Rate Limiting

Requests are rate-limited per IP address using a token bucket algorithm.

- **Default:** 100 requests per minute
- **Burst:** ~16 requests (10 second burst)
- **Response:** HTTP 429 when rate limit exceeded

Configure via `RATE_LIMIT_PER_MINUTE` environment variable.

---

## DVM Interface (NIP-90)

WoT Oracle can also respond to Nostr DVM (Data Vending Machine) requests.

**Request Event (kind 5950):**
```json
{
  "kind": 5950,
  "tags": [
    ["i", "<from_pubkey>", "text"],
    ["param", "target", "<to_pubkey>"],
    ["param", "max_hops", "5"]
  ]
}
```

**Response Event (kind 6950):**
```json
{
  "kind": 6950,
  "tags": [
    ["e", "<request_id>"],
    ["p", "<requester_pubkey>"],
    ["result", "hops", "2"],
    ["result", "path_count", "3"],
    ["result", "mutual_follow", "false"]
  ],
  "content": "{\"from\":\"...\",\"to\":\"...\",\"hops\":2,...}"
}
```

Enable DVM with `DVM_ENABLED=true` and `DVM_PRIVATE_KEY=<nsec or hex>`.

---

## Caching

Query results are cached in an LRU cache with configurable size and TTL.

- **Cache Key:** (from_id, to_id, max_hops, include_bridges)
- **Invalidation:** Cache entries are invalidated when either node's follow list changes

Use `bypass_cache=true` to force fresh computation.
