mod input;
mod server;
use crate::server::MyServer;
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use rusqlite::Connection;
use russh::keys::ssh_key::rand_core::OsRng;
use russh::server::Server as _;
use shootsh_core::db::{DbCache, DbRequest, Repository};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

const DEFAULT_MAX_USERS: i64 = 100_000;

#[tokio::main]
async fn main() -> Result<()> {
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
            println!("Current active connections: {}", count);
        }
    });

    let config = Arc::new(russh::server::Config {
        inactivity_timeout: Some(Duration::from_secs(60 * 10)),
        auth_rejection_time: Duration::from_secs(3),
        nodelay: true,
        keys: vec![
            russh::keys::PrivateKey::random(&mut OsRng, russh::keys::Algorithm::Ed25519)
                .map_err(|e| anyhow::anyhow!("Key gen failed: {}", e))?,
        ],
        ..Default::default()
    });

    let mut sh = MyServer {
        db_tx,
        shared_cache,
        connection_count,
        active_sessions: Arc::new(std::sync::Mutex::new(HashMap::new())),
    };

    let addr = "0.0.0.0:2222";
    let socket = TcpListener::bind(addr).await?;
    println!("Starting shootsh_ssh on {}", addr);

    tokio::select! {
            res = sh.run_on_socket(config, &socket) => {
                if let Err(e) = res {
                    eprintln!("Server error: {:?}", e);
                }
            },
    _ = tokio::signal::ctrl_c() => {
                println!("\n[!] Shutdown signal received. Cleaning up sessions...");
                sh.cleanup_all_sessions().await;

                // wait for cleanup
                tokio::time::sleep(Duration::from_millis(500)).await;
                println!("[!] Cleanup complete. Exiting.");
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
        while let Some(req) = rx.blocking_recv() {
            match repo.handle_request(req) {
                Some(new_cache) => {
                    cache.store(Arc::new(new_cache));
                }
                None => {}
            }
        }
    });
}
