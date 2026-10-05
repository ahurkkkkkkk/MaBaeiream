mod api;

use anyhow::{Context, bail};
use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString},
};
use clap::{Parser, Subcommand};
use rand_core::OsRng;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{path::PathBuf, str::FromStr};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "mabaeiream-server",
    version,
    about = "Private media server for MaBaeiream clients"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve,
    AddUser { username: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "mabaeiream_server=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let db_path = env_path("MABAEIREAM_DB", "data/mabaeiream.sqlite3");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create database directory {}", parent.display()))?;
    }
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .context("open SQLite database")?;
    migrate(&pool).await?;

    match cli.command {
        Command::AddUser { username } => add_user(&pool, username).await,
        Command::Serve => serve(pool).await,
    }
}

fn env_path(key: &str, fallback: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(fallback))
}

async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            username TEXT NOT NULL UNIQUE COLLATE NOCASE,
            password_hash TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sessions (
            token_hash TEXT PRIMARY KEY,
            user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            expires_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS sessions_expiry ON sessions(expires_at)")
        .execute(pool)
        .await?;
    Ok(())
}

async fn add_user(pool: &SqlitePool, username: String) -> anyhow::Result<()> {
    let username = username.trim();
    if username.is_empty() || username.len() > 64 {
        bail!("username must contain 1–64 characters");
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    if count >= 2 {
        bail!("MaBaeiream is limited to two accounts");
    }

    let password = rpassword::prompt_password("New password (12 characters minimum): ")?;
    if password.chars().count() < 12 {
        bail!("password must be at least 12 characters");
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| anyhow::Error::msg(error.to_string()))?
        .to_string();
    let now = unix_now();

    sqlx::query("INSERT INTO users(username, password_hash, created_at) VALUES(?, ?, ?)")
        .bind(username)
        .bind(hash)
        .bind(now)
        .execute(pool)
        .await
        .context("create account (username may already exist)")?;
    println!("Created MaBaeiream account for {username}.");
    Ok(())
}

async fn serve(pool: SqlitePool) -> anyhow::Result<()> {
    let media_path = env_path("MABAEIREAM_MEDIA_DIR", "media");
    std::fs::create_dir_all(&media_path)
        .with_context(|| format!("create media directory {}", media_path.display()))?;
    let media_root = tokio::fs::canonicalize(&media_path)
        .await
        .with_context(|| format!("resolve media directory {}", media_path.display()))?;

    let listen = std::env::var("MABAEIREAM_LISTEN").unwrap_or_else(|_| "127.0.0.1:9781".to_owned());
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .with_context(|| format!("bind {listen}"))?;

    tracing::info!(address = %listener.local_addr()?, media = %media_root.display(), "MaBaeiream server ready");
    axum::serve(listener, api::router(pool, media_root))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("serve HTTP API")
}

pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(crate) fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
