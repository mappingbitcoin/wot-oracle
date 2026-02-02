use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub relays: Vec<String>,
    pub http_port: u16,
    pub db_path: String,
    pub dvm_enabled: bool,
    pub dvm_private_key: Option<String>,
    pub rate_limit_per_minute: u32,
    pub max_hops: u8,
    pub cache_size: usize,
    pub cache_ttl_secs: u64,
}

impl Config {
    pub fn from_env() -> Self {
        let relays = env::var("RELAYS")
            .unwrap_or_else(|_| "wss://relay.damus.io,wss://nos.lol,wss://relay.nostr.band".into())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let http_port = env::var("HTTP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);

        let db_path = env::var("DB_PATH").unwrap_or_else(|_| "wot.db".into());

        let dvm_enabled = env::var("DVM_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let dvm_private_key = env::var("DVM_PRIVATE_KEY").ok();

        let rate_limit_per_minute = env::var("RATE_LIMIT_PER_MINUTE")
            .ok()
            .and_then(|r| r.parse().ok())
            .unwrap_or(100);

        let max_hops = env::var("MAX_HOPS")
            .ok()
            .and_then(|h| h.parse().ok())
            .unwrap_or(3);

        let cache_size = env::var("CACHE_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10000);

        let cache_ttl_secs = env::var("CACHE_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(300);

        Self {
            relays,
            http_port,
            db_path,
            dvm_enabled,
            dvm_private_key,
            rate_limit_per_minute,
            max_hops,
            cache_size,
            cache_ttl_secs,
        }
    }
}
