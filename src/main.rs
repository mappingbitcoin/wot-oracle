mod api;
mod cache;
mod config;
mod db;
mod graph;
mod sync;

use anyhow::Result;
use std::sync::Arc;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use api::{http::AppState, DvmService};
use cache::QueryCache;
use config::Config;
use db::Database;
use graph::WotGraph;
use sync::Ingestion;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("WoT Oracle v{} starting...", env!("CARGO_PKG_VERSION"));
    info!(
        "Tokio runtime: {} worker threads",
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
    );

    // Load configuration
    let config = Config::from_env();
    info!("Configuration loaded: {} relays, HTTP port {}", config.relays.len(), config.http_port);

    // Initialize database
    let db = Arc::new(Database::open(&config.db_path)?);
    info!("Database opened at: {}", config.db_path);

    // Create graph and load from database
    let graph = Arc::new(WotGraph::new());
    db.load_graph(&graph)?;

    let initial_stats = graph.stats();
    info!(
        "Graph loaded: {} nodes, {} edges",
        initial_stats.node_count, initial_stats.edge_count
    );

    // Create shared config
    let config = Arc::new(config);

    // Create query cache
    let cache = Arc::new(QueryCache::new(config.cache_size, config.cache_ttl_secs));
    info!(
        "Query cache initialized: {} entries, {} second TTL",
        config.cache_size, config.cache_ttl_secs
    );

    // Create app state for HTTP server
    let app_state = AppState {
        graph: graph.clone(),
        config: config.clone(),
        cache: cache.clone(),
    };

    // Start ingestion daemon
    let ingestion = Ingestion::new(graph.clone(), db.clone(), config.relays.clone());
    let ingestion_handle = tokio::spawn(async move {
        if let Err(e) = ingestion.start().await {
            error!("Ingestion error: {}", e);
        }
    });

    // Start DVM service if enabled
    let _dvm_handle = if config.dvm_enabled {
        if let Some(ref private_key) = config.dvm_private_key {
            match DvmService::new(graph.clone(), cache.clone(), config.clone(), private_key) {
                Ok(dvm) => {
                    let handle = tokio::spawn(async move {
                        if let Err(e) = dvm.start().await {
                            error!("DVM error: {}", e);
                        }
                    });
                    Some(handle)
                }
                Err(e) => {
                    error!("Failed to create DVM service: {}", e);
                    None
                }
            }
        } else {
            error!("DVM enabled but DVM_PRIVATE_KEY not set");
            None
        }
    } else {
        info!("DVM service disabled");
        None
    };

    // Start HTTP server
    let http_port = config.http_port;
    let rate_limit = config.rate_limit_per_minute;
    let http_handle = tokio::spawn(async move {
        if let Err(e) = api::http::start_server(app_state, http_port, rate_limit).await {
            error!("HTTP server error: {}", e);
        }
    });

    // Wait for shutdown signal
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
        }
        _ = http_handle => {
            error!("HTTP server terminated unexpectedly");
        }
        _ = ingestion_handle => {
            error!("Ingestion daemon terminated unexpectedly");
        }
    }

    info!("Shutting down...");
    Ok(())
}
