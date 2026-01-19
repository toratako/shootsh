mod input;
mod server;

use crate::server::MyServer;
use anyhow::{Context, Result};
use rusqlite::Connection;
use russh::keys::ssh_key::rand_core::OsRng;
use russh::server::Server as _;
use shootsh_core::db::{DbCache, DbRequest, Repository};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    let conn = Connection::open("shootsh.db").context("Failed to open DB")?;
    let repo = Repository::new(conn).context("Failed to init repo")?;

    let shared_cache = Arc::new(Mutex::new(Arc::new(repo.get_current_cache())));

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
        inactivity_timeout: Some(Duration::from_secs(3600)),
        auth_rejection_time: Duration::from_secs(3),
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
    };

    let addr = "0.0.0.0:2222";
    let socket = TcpListener::bind(addr).await?;
    println!("Starting shootsh_ssh on {}", addr);

    sh.run_on_socket(config, &socket).await?;
    Ok(())
}

fn spawn_db_worker(
    repo: Repository,
    cache: Arc<Mutex<Arc<DbCache>>>,
    mut rx: mpsc::Receiver<DbRequest>,
) {
    std::thread::spawn(move || {
        while let Some(req) = rx.blocking_recv() {
            if let Some(new_cache) = repo.handle_request(req) {
                let new_arc = Arc::new(new_cache);
                if let Ok(mut lock) = cache.lock() {
                    *lock = new_arc;
                }
            }
        }
    });
}
