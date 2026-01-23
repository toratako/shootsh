mod input;
mod server;
use crate::server::MyServer;
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use rusqlite::Connection;
use russh::keys::load_secret_key;
use russh::server::Server as _;
use shootsh_core::db::{DbCache, DbRequest, Repository};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

const DEFAULT_MAX_USERS: i64 = 100_000;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(true))
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    tracing::info!("Starting shootsh_ssh server...");

    let conn = Connection::open("shootsh.db").context("Failed to open DB")?;
    let repo = Repository::new(conn, DEFAULT_MAX_USERS).context("Failed to init repo")?;
    let shared_cache = Arc::new(ArcSwap::from_pointee(repo.get_current_cache()));
    let (db_tx, db_rx) = mpsc::channel::<DbRequest>(100);
    spawn_db_worker(repo, Arc::clone(&shared_cache), db_rx);

    let connection_count = Arc::new(AtomicUsize::new(0));
    let count_for_log = Arc::clone(&connection_count);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let count = count_for_log.load(Ordering::Relaxed);
            tracing::info!(active_connections = count, "Connection stats");
        }
    });

    let key_path = env::var("SSH_HOST_KEY_PATH").context("SSH_HOST_KEY_PATH is not set")?;
    let host_key = load_secret_key(key_path, None).context("Failed to load SSH host key")?;

    let config = Arc::new(russh::server::Config {
        inactivity_timeout: Some(Duration::from_secs(60 * 10)),
        auth_rejection_time: Duration::from_secs(3),
        nodelay: true,
        keys: vec![host_key],
        ..Default::default()
    });

    let sh = MyServer {
        db_tx,
        shared_cache,
        connection_count,
        active_sessions: Arc::new(std::sync::Mutex::new(HashMap::new())),
    };

    let addr = "0.0.0.0:2222";
    let socket = TcpListener::bind(addr).await?;
    tracing::info!(listen_addr = %addr, "SSH server listening");

    let mut sh_clone = sh.clone();
    let server_task = tokio::spawn(async move { sh_clone.run_on_socket(config, &socket).await });

    tokio::select! {
        res = server_task => {
            match res {
                Ok(Ok(())) => tracing::info!("Server stopped normally"),
                Ok(Err(e)) => tracing::error!(error = ?e, "Server error occurred"),
                Err(e) => tracing::error!(error = ?e, "Server task panicked"),
            }
        },
        _ = tokio::signal::ctrl_c() => {
            tracing::warn!("Shutdown signal received. Starting cleanup...");

            sh.cleanup_all_sessions().await;

            // wait for cleanup
            tokio::time::sleep(Duration::from_millis(500)).await;
            tracing::info!("Graceful shutdown complete");
        }
    }

    Ok(())
}

fn spawn_db_worker(
    repo: Repository,
    cache: Arc<ArcSwap<DbCache>>,
    mut rx: mpsc::Receiver<DbRequest>,
) {
    std::thread::spawn(move || {
        let span = tracing::info_span!("db_worker");
        let _enter = span.enter();
        tracing::info!("DB worker thread started");

        while let Some(req) = rx.blocking_recv() {
            tracing::debug!(request = ?req, "Handling DB request");
            match repo.handle_request(req) {
                Some(new_cache) => {
                    cache.store(Arc::new(new_cache));
                    tracing::debug!("DB cache updated");
                }
                None => {}
            }
        }
        tracing::info!("DB worker thread shutting down");
    });
}
