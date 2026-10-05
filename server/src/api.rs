use crate::{unix_now, unix_now_ms};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json, Router,
    body::Body,
    extract::{
        DefaultBodyLimit, Extension, Path as AxumPath, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, Request, Response, StatusCode, header},
    middleware::{self, Next},
    routing::{delete, get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::{Mutex, Semaphore, broadcast},
};
use tokio_util::io::ReaderStream;
use url::Url;

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    media_root: Arc<PathBuf>,
    downloads: Arc<Mutex<HashMap<String, DownloadJob>>>,
    download_slot: Arc<Semaphore>,
    download_tasks: Arc<Mutex<HashMap<String, tokio::task::AbortHandle>>>,
    upload_slots: Arc<Semaphore>,
    media_http: reqwest::Client,
    room: Arc<Mutex<RoomState>>,
    room_events: broadcast::Sender<RoomState>,
    voice_signals: broadcast::Sender<AuthenticatedSignal>,
    room_connections: Arc<Semaphore>,
}

#[derive(Clone)]
struct SessionUser {
    _id: i64,
    _username: String,
    token_hash: String,
}

#[derive(Deserialize)]
struct LoginInput {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginOutput {
    token: String,
    username: String,
    expires_at: i64,
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    path: String,
}

#[derive(Deserialize)]
struct SubtitleQuery {
    media: String,
}

#[derive(Serialize)]
struct SubtitleResponse {
    subtitles: Vec<SubtitleFile>,
}

#[derive(Serialize)]
struct SubtitleFile {
    name: String,
    path: String,
    mime_type: &'static str,
}

#[derive(Serialize)]
struct MediaResponse {
    path: String,
    items: Vec<MediaItem>,
}

#[derive(Serialize)]
struct MediaItem {
    name: String,
    path: String,
    kind: &'static str,
    size: u64,
}

#[derive(Clone, Serialize)]
struct DownloadJob {
    id: String,
    #[serde(skip)]
    owner_id: i64,
    state: String,
    bytes: u64,
    total: Option<u64>,
    saved_as: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct DownloadInput {
    url: String,
}

#[derive(Deserialize)]
struct ResolveInput {
    url: String,
}

#[derive(Deserialize)]
struct UploadQuery {
    #[serde(default)]
    path: String,
    name: String,
}

#[derive(Deserialize)]
struct CreateFolderInput {
    #[serde(default)]
    path: String,
    name: String,
}

#[derive(Serialize)]
struct CreatedEntry {
    name: String,
    path: String,
}

#[derive(Serialize)]
struct UploadResponse {
    name: String,
    path: String,
    size: u64,
}

#[derive(Serialize)]
struct LiveConfig {
    stream_path: String,
    whip_url: String,
    rtmps_server_url: String,
    stream_key: String,
    hls_url: String,
}

#[derive(Serialize)]
struct LiveStatus {
    active: bool,
}

#[derive(Serialize)]
struct VoiceIceResponse {
    ice_servers: Vec<VoiceIceServer>,
    expires_at: i64,
}

#[derive(Serialize)]
struct VoiceIceServer {
    urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    credential: Option<String>,
}

#[derive(Deserialize)]
struct MediaMtxAuth {
    #[serde(default)]
    token: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    protocol: String,
}

#[derive(Serialize)]
struct ResolveOutput {
    url: String,
    audio_url: Option<String>,
    title: String,
    headers: HashMap<String, String>,
}

const ROOM_MAX_MESSAGE_BYTES: usize = 40 * 1024;
const ROOM_RATE_WINDOW: Duration = Duration::from_secs(10);
const ROOM_MAX_MESSAGES_PER_WINDOW: u32 = 120;
const ROOM_MAX_CONNECTIONS: usize = 16;
const ROOM_MAX_POSITION_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const VOICE_MAX_SDP_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug)]
struct RoomSource {
    kind: &'static str,
    source: String,
    title: String,
}

#[derive(Clone, Debug)]
struct RoomState {
    revision: u64,
    source: Option<RoomSource>,
    playing: bool,
    position_ms: u64,
    position_anchor_ms: i64,
    updated_by: Option<String>,
}

impl Default for RoomState {
    fn default() -> Self {
        Self {
            revision: 0,
            source: None,
            playing: false,
            position_ms: 0,
            position_anchor_ms: unix_now_ms(),
            updated_by: None,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum RoomCommand {
    #[serde(rename = "select")]
    Select {
        source_kind: String,
        source: String,
        title: String,
        #[serde(default)]
        playing: bool,
    },
    #[serde(rename = "play")]
    Play { position_ms: u64 },
    #[serde(rename = "pause")]
    Pause { position_ms: u64 },
    #[serde(rename = "seek")]
    Seek { position_ms: u64 },
}

#[derive(Deserialize)]
struct RoomMessageEnvelope {
    #[serde(rename = "type")]
    message_type: String,
}

#[derive(Deserialize)]
struct SignalEnvelope {
    #[serde(rename = "type")]
    message_type: String,
    kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OfferSignalInput {
    #[serde(rename = "type")]
    message_type: SignalMessageType,
    kind: OfferKind,
    sdp: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnswerSignalInput {
    #[serde(rename = "type")]
    message_type: SignalMessageType,
    kind: AnswerKind,
    sdp: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateSignalInput {
    #[serde(rename = "type")]
    message_type: SignalMessageType,
    kind: CandidateKind,
    candidate: String,
    sdp_mid: Option<String>,
    sdp_mline_index: Option<u16>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndSignalInput {
    #[serde(rename = "type")]
    message_type: SignalMessageType,
    kind: EndKind,
}

#[derive(Deserialize)]
enum SignalMessageType {
    #[serde(rename = "signal")]
    Signal,
}

#[derive(Deserialize)]
enum OfferKind {
    #[serde(rename = "offer")]
    Offer,
}

#[derive(Deserialize)]
enum AnswerKind {
    #[serde(rename = "answer")]
    Answer,
}

#[derive(Deserialize)]
enum CandidateKind {
    #[serde(rename = "candidate")]
    Candidate,
}

#[derive(Deserialize)]
enum EndKind {
    #[serde(rename = "end")]
    End,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum ValidatedSignal {
    Offer {
        sdp: String,
    },
    Answer {
        sdp: String,
    },
    Candidate {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
    },
    End,
}

#[derive(Clone)]
struct AuthenticatedSignal {
    sender_id: i64,
    sender_session_hash: String,
    from: String,
    signal: ValidatedSignal,
}

impl AuthenticatedSignal {
    fn should_deliver_to(&self, recipient_id: i64, recipient_session_hash: &str) -> bool {
        self.sender_id != recipient_id || self.sender_session_hash != recipient_session_hash
    }

    fn message(&self) -> serde_json::Value {
        let mut message = serde_json::to_value(&self.signal)
            .expect("validated voice signaling messages are serializable");
        let fields = message
            .as_object_mut()
            .expect("tagged signaling messages serialize as JSON objects");
        fields.insert("type".into(), serde_json::json!("signal"));
        fields.insert("from".into(), serde_json::json!(self.from));
        message
    }
}

enum RoomMessage {
    Playback(RoomCommand),
    Signal(ValidatedSignal),
}

enum ValidatedRoomCommand {
    Select { source: RoomSource, playing: bool },
    Play(u64),
    Pause(u64),
    Seek(u64),
}

impl RoomState {
    fn current_position_ms(&self, now_ms: i64) -> u64 {
        if self.playing {
            self.position_ms
                .saturating_add(now_ms.saturating_sub(self.position_anchor_ms).max(0) as u64)
        } else {
            self.position_ms
        }
    }

    fn state_message(&self, now_ms: i64) -> serde_json::Value {
        serde_json::json!({
            "type": "state",
            "revision": self.revision,
            "source_kind": self.source.as_ref().map(|source| source.kind),
            "source": self.source.as_ref().map(|source| source.source.as_str()),
            "title": self.source.as_ref().map(|source| source.title.as_str()),
            "playing": self.playing,
            "position_ms": self.current_position_ms(now_ms),
            "server_time_ms": now_ms,
            "updated_by": self.updated_by,
        })
    }

    fn apply(
        &mut self,
        command: ValidatedRoomCommand,
        username: &str,
        now_ms: i64,
    ) -> Result<(), &'static str> {
        match command {
            ValidatedRoomCommand::Select { source, playing } => {
                self.source = Some(source);
                self.playing = playing;
                self.position_ms = 0;
            }
            ValidatedRoomCommand::Play(position_ms) => {
                if self.source.is_none() {
                    return Err("Select a video before starting playback.");
                }
                self.position_ms = position_ms;
                self.playing = true;
            }
            ValidatedRoomCommand::Pause(position_ms) => {
                if self.source.is_none() {
                    return Err("Select a video before changing playback.");
                }
                self.position_ms = position_ms;
                self.playing = false;
            }
            ValidatedRoomCommand::Seek(position_ms) => {
                if self.source.is_none() {
                    return Err("Select a video before changing playback.");
                }
                self.position_ms = position_ms;
            }
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("The room revision limit has been reached.")?;
        self.position_anchor_ms = now_ms;
        self.updated_by = Some(username.to_owned());
        Ok(())
    }
}

pub fn router(db: SqlitePool, media_root: PathBuf) -> Router {
    let (room_events, _) = broadcast::channel(32);
    let (voice_signals, _) = broadcast::channel(64);
    let state = AppState {
        db,
        media_root: Arc::new(media_root),
        downloads: Arc::new(Mutex::new(HashMap::new())),
        download_slot: Arc::new(Semaphore::new(1)),
        download_tasks: Arc::new(Mutex::new(HashMap::new())),
        upload_slots: Arc::new(Semaphore::new(2)),
        media_http: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("media proxy HTTP client configuration is valid"),
        room: Arc::new(Mutex::new(RoomState::default())),
        room_events,
        voice_signals,
        room_connections: Arc::new(Semaphore::new(ROOM_MAX_CONNECTIONS)),
    };
    let public = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/auth/login", post(login))
        .route("/internal/mediamtx/auth", post(mediamtx_auth));
    let protected = Router::new()
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/media", get(list_media))
        .route("/api/v1/subtitles", get(list_subtitles))
        .route("/api/v1/stream", get(stream_media))
        .route("/api/v1/uploads", post(upload_media))
        .route("/api/v1/folders", post(create_folder))
        .route("/api/v1/live/config", get(live_config))
        .route("/api/v1/live/status", get(live_status))
        .route("/api/v1/voice/ice", get(voice_ice_config))
        .route("/api/v1/live/hls/{*path}", get(live_hls))
        .route("/api/v1/media/resolve", post(resolve_media))
        .route("/api/v1/downloads", post(start_download))
        .route("/api/v1/downloads/{id}", get(download_status))
        .route("/api/v1/downloads/{id}/cancel", post(cancel_download))
        .route("/api/v1/downloads/{id}", delete(cancel_download))
        .route("/api/v1/room/ws", get(room_ws))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ));
    public
        .merge(protected)
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "mabaeiream" }))
}

async fn live_config() -> Result<Json<LiveConfig>, StatusCode> {
    let key = live_stream_key().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let base = std::env::var("MABAEIREAM_PUBLIC_BASE_URL")
        .unwrap_or_else(|_| "https://ahura.site/mabaeiream".to_owned());
    let base_url =
        Url::parse(base.trim_end_matches('/')).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if base_url.scheme() != "https"
        || base_url.host_str().is_none()
        || !base_url.username().is_empty()
        || base_url.password().is_some()
        || base_url.query().is_some()
        || base_url.fragment().is_some()
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let stream_path = format!("mabaeiream/{key}");
    let public = base.trim_end_matches('/');
    let rtmps_server_url = std::env::var("MABAEIREAM_RTMPS_SERVER_URL")
        .unwrap_or_else(|_| "rtmps://ahura.site:1936/mabaeiream".to_owned());
    let hls_url = format!("{public}/api/v1/live/hls/{stream_path}/index.m3u8");
    Ok(Json(LiveConfig {
        stream_path: stream_path.clone(),
        whip_url: format!("{public}/live/{stream_path}/whip"),
        rtmps_server_url,
        stream_key: key,
        hls_url,
    }))
}

async fn voice_ice_config(
    Extension(user): Extension<SessionUser>,
) -> Result<Json<VoiceIceResponse>, StatusCode> {
    let urls = std::env::var("MABAEIREAM_TURN_URLS").unwrap_or_default();
    let secret = std::env::var("MABAEIREAM_TURN_SECRET").ok();
    let expires_at = unix_now().saturating_add(VOICE_TURN_CREDENTIAL_TTL_SECS);
    let turn_servers = make_turn_servers(&urls, secret.as_deref(), user._id, expires_at)?;
    let mut ice_servers = vec![VoiceIceServer {
        urls: vec!["stun:stun.l.google.com:19302".to_owned()],
        username: None,
        credential: None,
    }];
    ice_servers.extend(turn_servers);
    Ok(Json(VoiceIceResponse {
        ice_servers,
        expires_at,
    }))
}

const VOICE_TURN_CREDENTIAL_TTL_SECS: i64 = 12 * 60 * 60;

fn make_turn_servers(
    urls: &str,
    secret: Option<&str>,
    user_id: i64,
    expires_at: i64,
) -> Result<Vec<VoiceIceServer>, StatusCode> {
    let urls: Vec<String> = urls
        .split(',')
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
        .collect();
    if urls.is_empty() {
        return Ok(Vec::new());
    }
    let Some(secret) = secret.filter(|secret| secret.as_bytes().len() >= 32) else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    if urls.len() > 8
        || urls.iter().any(|url| {
            url.len() > 256
                || url.chars().any(char::is_whitespace)
                || !(url.starts_with("turn:") || url.starts_with("turns:"))
        })
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let username = format!("{expires_at}:{user_id}");
    let mut mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    mac.update(username.as_bytes());
    let credential = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    Ok(vec![VoiceIceServer {
        urls,
        username: Some(username),
        credential: Some(credential),
    }])
}

async fn live_status(State(state): State<AppState>) -> Result<Json<LiveStatus>, StatusCode> {
    let Some(path) = live_ingest_path() else {
        return Ok(Json(LiveStatus { active: false }));
    };
    let response = state
        .media_http
        .get("http://127.0.0.1:9997/v3/paths/list")
        .send()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if !response.status().is_success() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let data = response
        .json::<serde_json::Value>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let active = data
        .get("items")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("name").and_then(serde_json::Value::as_str) == Some(path.as_str())
                    && item.get("ready").and_then(serde_json::Value::as_bool) == Some(true)
            })
        });
    Ok(Json(LiveStatus { active }))
}

async fn live_hls(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
    request: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let expected = live_ingest_path().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    if !is_safe_live_hls_path(&path, &expected) {
        return Err(StatusCode::NOT_FOUND);
    }
    let mut upstream_url =
        Url::parse("http://127.0.0.1:8888/").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    {
        let mut segments = upstream_url
            .path_segments_mut()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        segments.pop_if_empty();
        for segment in path.split('/') {
            segments.push(segment);
        }
    }
    upstream_url.set_query(request.uri().query());
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let upstream = state
        .media_http
        .get(upstream_url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).map_err(|_| StatusCode::BAD_GATEWAY)?;
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let cache_control = upstream.headers().get(header::CACHE_CONTROL).cloned();
    let stream = upstream.bytes_stream();
    let mut response = Response::builder().status(status);
    if let Some(value) = content_type {
        response = response.header(header::CONTENT_TYPE, value);
    }
    if let Some(value) = cache_control {
        response = response.header(header::CACHE_CONTROL, value);
    }
    response
        .body(Body::from_stream(stream))
        .map_err(|_| StatusCode::BAD_GATEWAY)
}

async fn mediamtx_auth(
    State(state): State<AppState>,
    Json(input): Json<MediaMtxAuth>,
) -> StatusCode {
    let Some(expected_path) = live_ingest_path() else {
        return StatusCode::UNAUTHORIZED;
    };
    if input.path != expected_path {
        return StatusCode::UNAUTHORIZED;
    }
    if input.action == "publish" {
        return if matches!(input.protocol.as_str(), "rtmp" | "webrtc") {
            StatusCode::OK
        } else {
            StatusCode::UNAUTHORIZED
        };
    }
    if input.action != "read" || input.protocol != "hls" || input.token.len() < 32 {
        return StatusCode::UNAUTHORIZED;
    }
    match sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sessions WHERE token_hash = ? AND expires_at > ?",
    )
    .bind(hex_digest(input.token.as_bytes()))
    .bind(unix_now())
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(_)) => StatusCode::OK,
        _ => StatusCode::UNAUTHORIZED,
    }
}

fn live_stream_key() -> Option<String> {
    std::env::var("MABAEIREAM_LIVE_STREAM_KEY")
        .ok()
        .filter(|key| {
            (32..=128).contains(&key.len())
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
}

fn live_ingest_path() -> Option<String> {
    live_stream_key().map(|key| format!("mabaeiream/{key}"))
}

fn is_safe_live_hls_path(path: &str, expected: &str) -> bool {
    let Some(suffix) = path.strip_prefix(&format!("{expected}/")) else {
        return false;
    };
    !suffix.is_empty()
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        })
}

async fn resolve_media(Json(input): Json<ResolveInput>) -> Result<Json<ResolveOutput>, StatusCode> {
    let webpage = validate_resolvable_provider_url(&input.url)?;
    let ytdlp = std::env::var_os("MABAEIREAM_YTDLP").unwrap_or_else(|| "yt-dlp".into());
    let mut command = tokio::process::Command::new(ytdlp);
    command
        .args([
            "--no-warnings",
            "--no-progress",
            "--no-playlist",
            "--dump-single-json",
            "--format",
            "best[ext=mp4][vcodec!=none][acodec!=none]/best[vcodec!=none][acodec!=none]/bestvideo[ext=mp4][vcodec^=avc1]+bestaudio[ext=m4a]/bestvideo[ext=mp4]+bestaudio[ext=m4a]/bestvideo+bestaudio/bestvideo[ext=mp4][vcodec^=avc1]/bestvideo[ext=mp4]/bestvideo",
            "--",
        ])
        .arg(webpage.as_str())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(25), command.output())
        .await
        .map_err(|_| StatusCode::GATEWAY_TIMEOUT)?
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if !output.status.success() || output.stdout.len() > 1_048_576 {
        return Err(StatusCode::BAD_GATEWAY);
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|_| StatusCode::BAD_GATEWAY)?;
    let (stream_url, audio_url) = select_resolved_streams(&metadata)?;
    let title = metadata
        .get("title")
        .and_then(serde_json::Value::as_str)
        .filter(|value| {
            !value.trim().is_empty()
                && value.chars().count() <= 160
                && !value.chars().any(char::is_control)
        })
        .unwrap_or("Shared video")
        .to_owned();
    let mut headers = HashMap::new();
    if let Some(values) = metadata
        .get("http_headers")
        .and_then(serde_json::Value::as_object)
    {
        for name in [
            "user-agent",
            "referer",
            "origin",
            "accept",
            "accept-language",
        ] {
            if let Some(value) = values.iter().find_map(|(key, value)| {
                key.eq_ignore_ascii_case(name)
                    .then(|| value.as_str())
                    .flatten()
                    .filter(|value| value.len() <= 1_024 && !value.contains(['\r', '\n']))
            }) {
                let canonical_name = match name {
                    "user-agent" => "User-Agent",
                    "referer" => "Referer",
                    "origin" => "Origin",
                    "accept" => "Accept",
                    _ => "Accept-Language",
                };
                headers.insert(canonical_name.to_owned(), value.to_owned());
            }
        }
    }
    Ok(Json(ResolveOutput {
        url: stream_url,
        audio_url,
        title,
        headers,
    }))
}

fn select_resolved_streams(
    metadata: &serde_json::Value,
) -> Result<(String, Option<String>), StatusCode> {
    if let Some(url) = metadata
        .get("url")
        .and_then(serde_json::Value::as_str)
        .and_then(validate_stream_url)
    {
        return Ok((url, None));
    }
    let formats = metadata
        .get("requested_formats")
        .and_then(serde_json::Value::as_array)
        .ok_or(StatusCode::BAD_GATEWAY)?;
    let video = formats
        .iter()
        .find(|format| codec_present(format, "vcodec"))
        .and_then(|format| format.get("url"))
        .and_then(serde_json::Value::as_str)
        .and_then(validate_stream_url)
        .ok_or(StatusCode::BAD_GATEWAY)?;
    let audio = formats
        .iter()
        .find(|format| codec_present(format, "acodec") && !codec_present(format, "vcodec"))
        .and_then(|format| format.get("url"))
        .and_then(serde_json::Value::as_str)
        .and_then(validate_stream_url);
    Ok((video, audio))
}

fn codec_present(format: &serde_json::Value, name: &str) -> bool {
    format
        .get(name)
        .and_then(serde_json::Value::as_str)
        .is_some_and(|codec| !codec.is_empty() && codec != "none")
}

fn validate_stream_url(value: &str) -> Option<String> {
    if value.len() > 8_192 {
        return None;
    }
    let url = Url::parse(value).ok()?;
    (url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.to_string())
}

fn validate_resolvable_provider_url(value: &str) -> Result<Url, StatusCode> {
    let url = Url::parse(value).map_err(|_| StatusCode::BAD_REQUEST)?;
    let host = url
        .host_str()
        .ok_or(StatusCode::BAD_REQUEST)?
        .to_ascii_lowercase();
    let supported = host == "youtu.be"
        || host == "youtube.com"
        || host.ends_with(".youtube.com")
        || host == "vimeo.com"
        || host.ends_with(".vimeo.com");
    if url.scheme() != "https"
        || !supported
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(url)
}

async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginInput>,
) -> Result<Json<LoginOutput>, StatusCode> {
    if input.username.is_empty() || input.password.is_empty() || input.username.len() > 64 {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let user = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT id, username, password_hash FROM users WHERE username = ? COLLATE NOCASE",
    )
    .bind(input.username.trim())
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    let parsed = PasswordHash::new(&user.2).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Argon2::default()
        .verify_password(input.password.as_bytes(), &parsed)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let mut raw = [0_u8; 32];
    OsRng.fill_bytes(&mut raw);
    let token = URL_SAFE_NO_PAD.encode(raw);
    let expires_at = unix_now() + Duration::from_secs(60 * 60 * 24 * 30).as_secs() as i64;
    sqlx::query("INSERT INTO sessions(token_hash, user_id, expires_at) VALUES(?, ?, ?)")
        .bind(hex_digest(token.as_bytes()))
        .bind(user.0)
        .bind(expires_at)
        .execute(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(LoginOutput {
        token,
        username: user.1,
        expires_at,
    }))
}

async fn logout(
    State(state): State<AppState>,
    Extension(user): Extension<SessionUser>,
    headers: HeaderMap,
) -> StatusCode {
    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return StatusCode::NO_CONTENT;
    };
    let _ = sqlx::query("DELETE FROM sessions WHERE token_hash = ? AND user_id = ?")
        .bind(hex_digest(token.as_bytes()))
        .bind(user._id)
        .execute(&state.db)
        .await;
    StatusCode::NO_CONTENT
}

async fn require_session(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response<Body>, StatusCode> {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| value.len() >= 32)
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let token_hash = hex_digest(token.as_bytes());
    let row = sqlx::query_as::<_, (i64, String)>(
        "SELECT users.id, users.username FROM sessions
         JOIN users ON users.id = sessions.user_id
         WHERE sessions.token_hash = ? AND sessions.expires_at > ?",
    )
    .bind(&token_hash)
    .bind(unix_now())
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::UNAUTHORIZED)?;
    request.extensions_mut().insert(SessionUser {
        _id: row.0,
        _username: row.1,
        token_hash,
    });
    Ok(next.run(request).await)
}

async fn room_ws(
    State(state): State<AppState>,
    Extension(user): Extension<SessionUser>,
    ws: WebSocketUpgrade,
) -> Response<Body> {
    ws.max_message_size(ROOM_MAX_MESSAGE_BYTES)
        .max_frame_size(ROOM_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| async move {
            let permit = match state.room_connections.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    let mut socket = socket;
                    let _ = socket.send(Message::Close(None)).await;
                    return;
                }
            };
            room_socket(socket, state, user, permit).await;
        })
}

async fn room_socket(
    socket: WebSocket,
    state: AppState,
    user: SessionUser,
    _connection_permit: tokio::sync::OwnedSemaphorePermit,
) {
    let mut events = state.room_events.subscribe();
    let mut voice_signals = state.voice_signals.subscribe();
    let initial = state.room.lock().await.clone();
    let mut last_revision = initial.revision;
    let (mut sender, mut receiver) = socket.split();
    let initial_message = initial.state_message(unix_now_ms());
    if sender
        .send(Message::Text(initial_message.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    let mut window_started = Instant::now();
    let mut message_count = 0_u32;
    let mut session_check = tokio::time::interval(Duration::from_secs(60));
    let mut sync_tick = tokio::time::interval(Duration::from_millis(500));
    sync_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    sync_tick.tick().await;
    loop {
        tokio::select! {
            incoming = receiver.next() => {
                let Some(Ok(message)) = incoming else { return; };
                if window_started.elapsed() >= ROOM_RATE_WINDOW {
                    window_started = Instant::now();
                    message_count = 0;
                }
                message_count += 1;
                if message_count > ROOM_MAX_MESSAGES_PER_WINDOW {
                    send_room_error(&mut sender, "Room message rate limit exceeded.").await;
                    return;
                }
                match message {
                    Message::Text(text) => {
                        if text.len() > ROOM_MAX_MESSAGE_BYTES {
                            send_room_error(&mut sender, "Message is too large.").await;
                            return;
                        }

                        let message = match parse_room_message(text.as_str()) {
                            Ok(message) => message,
                            Err(_) => {
                                send_room_error(&mut sender, "Invalid room message.").await;
                                continue;
                            }
                        };
                        match message {
                            RoomMessage::Signal(signal) => {
                                if let Err(message) = validate_signal(&signal) {
                                    send_room_error(&mut sender, message).await;
                                    continue;
                                }
                                let _ = state.voice_signals.send(AuthenticatedSignal {
                                    sender_id: user._id,
                                    sender_session_hash: user.token_hash.clone(),
                                    from: user._username.clone(),
                                    signal,
                                });
                            }
                            RoomMessage::Playback(command) => {
                                let command = match validate_room_command(&state.media_root, command).await {
                                    Ok(command) => command,
                                    Err(message) => {
                                        send_room_error(&mut sender, message).await;
                                        continue;
                                    }
                                };

                                let now_ms = unix_now_ms();
                                let updated = {
                                    let mut room = state.room.lock().await;
                                    match room.apply(command, &user._username, now_ms) {
                                        Ok(()) => Ok(room.clone()),
                                        Err(message) => Err(message),
                                    }
                                };
                                match updated {
                                    Ok(room) => {
                                        // The sender is subscribed too, so it receives the same server state.
                                        let _ = state.room_events.send(room);
                                    }
                                    Err(message) => send_room_error(&mut sender, message).await,
                                }
                            }
                        }
                    }
                    Message::Ping(payload) => {
                        if sender.send(Message::Pong(payload)).await.is_err() { return; }
                    }
                    Message::Pong(_) => {}
                    Message::Close(_) | Message::Binary(_) => return,
                }
            }
            event = events.recv() => {
                let room = match event {
                    Ok(room) => room,
                    Err(broadcast::error::RecvError::Lagged(_)) => state.room.lock().await.clone(),
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                if room.revision <= last_revision { continue; }
                let message = Message::Text(room.state_message(unix_now_ms()).to_string().into());
                if sender.send(message).await.is_err() { return; }
                last_revision = room.revision;
            }
            signal = voice_signals.recv() => {
                let signal = match signal {
                    Ok(signal) => signal,
                    // Signaling is ephemeral; if a client falls behind, keep the socket usable.
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                if !signal.should_deliver_to(user._id, &user.token_hash) { continue; }
                let message = Message::Text(signal.message().to_string().into());
                if sender.send(message).await.is_err() { return; }
            }
            _ = sync_tick.tick() => {
                let room = state.room.lock().await.clone();
                let message = Message::Text(room.state_message(unix_now_ms()).to_string().into());
                if sender.send(message).await.is_err() { return; }
            }
            _ = session_check.tick() => {
                let active: Option<i64> = sqlx::query_scalar(
                    "SELECT 1 FROM sessions WHERE token_hash = ? AND expires_at > ?"
                )
                .bind(&user.token_hash)
                .bind(unix_now())
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten();
                if active.is_none() {
                    let _ = sender.send(Message::Close(None)).await;
                    return;
                }
            }
        }
    }
}

async fn send_room_error<S>(sender: &mut S, message: &str)
where
    S: futures_util::Sink<Message> + Unpin,
{
    let payload = serde_json::json!({ "type": "error", "message": message });
    let _ = sender.send(Message::Text(payload.to_string().into())).await;
}

fn parse_room_message(text: &str) -> Result<RoomMessage, serde_json::Error> {
    let envelope = serde_json::from_str::<RoomMessageEnvelope>(text)?;
    if envelope.message_type == "signal" {
        parse_signal_message(text).map(RoomMessage::Signal)
    } else {
        serde_json::from_str::<RoomCommand>(text).map(RoomMessage::Playback)
    }
}

fn parse_signal_message(text: &str) -> Result<ValidatedSignal, serde_json::Error> {
    let envelope = serde_json::from_str::<SignalEnvelope>(text)?;
    if envelope.message_type != "signal" {
        return Err(<serde_json::Error as serde::de::Error>::custom(
            "signal message type is invalid",
        ));
    }

    match envelope.kind.as_str() {
        "offer" => {
            let OfferSignalInput {
                message_type: SignalMessageType::Signal,
                kind: OfferKind::Offer,
                sdp,
            } = serde_json::from_str(text)?;
            Ok(ValidatedSignal::Offer { sdp })
        }
        "answer" => {
            let AnswerSignalInput {
                message_type: SignalMessageType::Signal,
                kind: AnswerKind::Answer,
                sdp,
            } = serde_json::from_str(text)?;
            Ok(ValidatedSignal::Answer { sdp })
        }
        "candidate" => {
            let CandidateSignalInput {
                message_type: SignalMessageType::Signal,
                kind: CandidateKind::Candidate,
                candidate,
                sdp_mid,
                sdp_mline_index,
            } = serde_json::from_str(text)?;
            Ok(ValidatedSignal::Candidate {
                candidate,
                sdp_mid,
                sdp_mline_index,
            })
        }
        "end" => {
            let EndSignalInput {
                message_type: SignalMessageType::Signal,
                kind: EndKind::End,
            } = serde_json::from_str(text)?;
            Ok(ValidatedSignal::End)
        }
        _ => Err(<serde_json::Error as serde::de::Error>::custom(
            "signal kind is invalid",
        )),
    }
}

fn validate_signal(signal: &ValidatedSignal) -> Result<(), &'static str> {
    match signal {
        ValidatedSignal::Offer { sdp } | ValidatedSignal::Answer { sdp } => {
            if sdp.is_empty()
                || sdp.len() > VOICE_MAX_SDP_BYTES
                || !sdp.starts_with("v=0")
                || !sdp
                    .bytes()
                    .all(|byte| !byte.is_ascii_control() || matches!(byte, b'\r' | b'\n' | b'\t'))
            {
                return Err("WebRTC session description is invalid or too large.");
            }
        }
        ValidatedSignal::Candidate {
            candidate, sdp_mid, ..
        } => {
            if candidate.is_empty()
                || candidate.len() > 2048
                || !candidate.starts_with("candidate:")
                || !candidate
                    .bytes()
                    .all(|byte| !byte.is_ascii_control() || matches!(byte, b'\r' | b'\n' | b'\t'))
                || sdp_mid
                    .as_ref()
                    .is_some_and(|mid| mid.len() > 128 || mid.contains('\0'))
            {
                return Err("WebRTC ICE candidate is invalid or too large.");
            }
        }
        ValidatedSignal::End => {}
    }
    Ok(())
}

async fn validate_room_command(
    media_root: &Path,
    command: RoomCommand,
) -> Result<ValidatedRoomCommand, &'static str> {
    match command {
        RoomCommand::Select {
            source_kind,
            source,
            title,
            playing,
        } => {
            let title = title.trim();
            if title.is_empty()
                || title.chars().count() > 160
                || title.chars().any(char::is_control)
            {
                return Err("Video title must be 1–160 printable characters.");
            }
            if source.is_empty() || source.len() > 4096 || source.chars().any(char::is_control) {
                return Err("Video source is empty or too long.");
            }
            let kind = match source_kind.as_str() {
                "library" => {
                    if source.contains('\\') {
                        return Err("Library paths must use forward slashes.");
                    }
                    let path = resolve_inside(media_root, &source)
                        .await
                        .map_err(|_| "Library video was not found.")?;
                    let metadata = fs::metadata(&path)
                        .await
                        .map_err(|_| "Library video was not found.")?;
                    if !metadata.is_file()
                        || !is_video(path.extension().and_then(|value| value.to_str()))
                    {
                        return Err("Choose a video file from the library.");
                    }
                    "library"
                }
                "https" => {
                    validate_https_source(&source)?;
                    "https"
                }
                "live" => {
                    if live_ingest_path().as_deref() != Some(source.as_str()) {
                        return Err("The live desktop stream is not configured.");
                    }
                    "live"
                }
                _ => return Err("Video source kind must be library, https, or live."),
            };
            Ok(ValidatedRoomCommand::Select {
                source: RoomSource {
                    kind,
                    source,
                    title: title.to_owned(),
                },
                playing,
            })
        }
        RoomCommand::Play { position_ms } => {
            validate_position(position_ms)?;
            Ok(ValidatedRoomCommand::Play(position_ms))
        }
        RoomCommand::Pause { position_ms } => {
            validate_position(position_ms)?;
            Ok(ValidatedRoomCommand::Pause(position_ms))
        }
        RoomCommand::Seek { position_ms } => {
            validate_position(position_ms)?;
            Ok(ValidatedRoomCommand::Seek(position_ms))
        }
    }
}

fn validate_https_source(source: &str) -> Result<(), &'static str> {
    let url = Url::parse(source).map_err(|_| "Video source must be a valid HTTPS URL.")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || has_url_user_info(source)
    {
        return Err("Video links must use HTTPS and cannot contain credentials.");
    }
    Ok(())
}

fn has_url_user_info(source: &str) -> bool {
    source
        .split_once("://")
        .and_then(|(_, remainder)| remainder.split(['/', '?', '#']).next())
        .is_none_or(|authority| authority.contains('@'))
}

fn validate_position(position_ms: u64) -> Result<(), &'static str> {
    if position_ms > ROOM_MAX_POSITION_MS {
        Err("Playback position is outside the supported range.")
    } else {
        Ok(())
    }
}

async fn list_media(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<MediaResponse>, StatusCode> {
    let folder = resolve_inside(&state.media_root, &query.path).await?;
    if !fs::metadata(&folder)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?
        .is_dir()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut entries = fs::read_dir(&folder)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut items = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        let file_type = entry
            .file_type()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if !file_type.is_file() && !file_type.is_dir() {
            continue;
        }
        let meta = entry
            .metadata()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let relative = entry
            .path()
            .strip_prefix(state.media_root.as_path())
            .map_err(|_| StatusCode::FORBIDDEN)?
            .to_string_lossy()
            .replace('\\', "/");
        items.push(MediaItem {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: relative,
            kind: if file_type.is_dir() {
                "folder"
            } else if is_video(entry.path().extension().and_then(|value| value.to_str())) {
                "video"
            } else {
                "file"
            },
            size: meta.len(),
        });
    }
    items.sort_by(|a, b| {
        a.kind
            .cmp(b.kind)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Json(MediaResponse {
        path: query.path,
        items,
    }))
}

async fn list_subtitles(
    State(state): State<AppState>,
    Query(query): Query<SubtitleQuery>,
) -> Result<Json<SubtitleResponse>, StatusCode> {
    let media = resolve_inside(&state.media_root, &query.media).await?;
    let metadata = fs::metadata(&media)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !metadata.is_file() || !is_video(media.extension().and_then(|value| value.to_str())) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let stem = media
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let parent = media.parent().ok_or(StatusCode::BAD_REQUEST)?;
    let mut entries = fs::read_dir(parent)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut subtitles = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        if !entry
            .file_type()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .is_file()
        {
            continue;
        }
        let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let entry_path = entry.path();
        let Some(extension) = entry_path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some(mime_type) = subtitle_mime_type(extension) else {
            continue;
        };
        if !is_sidecar_for(stem, &file_name) {
            continue;
        }
        let path = entry_path
            .strip_prefix(state.media_root.as_path())
            .map_err(|_| StatusCode::FORBIDDEN)?
            .to_string_lossy()
            .replace('\\', "/");
        subtitles.push(SubtitleFile {
            name: file_name,
            path,
            mime_type,
        });
    }
    subtitles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(Json(SubtitleResponse { subtitles }))
}

fn is_sidecar_for(video_stem: &str, candidate_name: &str) -> bool {
    let Some((candidate_stem, _)) = candidate_name.rsplit_once('.') else {
        return false;
    };
    let candidate_stem = candidate_stem.to_lowercase();
    let video_stem = video_stem.to_lowercase();
    candidate_stem == video_stem || candidate_stem.starts_with(&(video_stem + "."))
}

fn subtitle_mime_type(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "srt" => Some("application/x-subrip"),
        "vtt" => Some("text/vtt"),
        "ttml" | "xml" => Some("application/ttml+xml"),
        "ssa" | "ass" => Some("text/x-ssa"),
        _ => None,
    }
}

async fn upload_media(
    State(state): State<AppState>,
    Query(query): Query<UploadQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<UploadResponse>), StatusCode> {
    validate_entry_name(&query.name)?;
    let limit = std::env::var("MABAEIREAM_MAX_UPLOAD_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(20 * 1024 * 1024 * 1024);
    let expected = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if expected.is_some_and(|size| size > limit) {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let _upload = state
        .upload_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let directory = resolve_inside(&state.media_root, &query.path).await?;
    if !fs::metadata(&directory)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?
        .is_dir()
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    let destination = directory.join(&query.name);
    let temporary = directory.join(format!(".mabaeiream-upload-{}.part", uuid::Uuid::new_v4()));
    let _cleanup = RemoveOnDrop(temporary.clone());
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut received = 0_u64;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| StatusCode::BAD_REQUEST)?;
        received = received
            .checked_add(chunk.len() as u64)
            .filter(|size| *size <= limit)
            .ok_or(StatusCode::PAYLOAD_TOO_LARGE)?;
        file.write_all(&chunk)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if expected.is_some_and(|size| size != received) {
        return Err(StatusCode::BAD_REQUEST);
    }
    file.sync_all()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    drop(file);
    fs::hard_link(&temporary, &destination)
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        })?;
    let _ = fs::remove_file(&temporary).await;

    let relative = destination
        .strip_prefix(state.media_root.as_path())
        .map_err(|_| StatusCode::FORBIDDEN)?
        .to_string_lossy()
        .replace('\\', "/");
    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            name: query.name,
            path: relative,
            size: received,
        }),
    ))
}

async fn create_folder(
    State(state): State<AppState>,
    Json(input): Json<CreateFolderInput>,
) -> Result<(StatusCode, Json<CreatedEntry>), StatusCode> {
    validate_entry_name(&input.name)?;
    let parent = resolve_inside(&state.media_root, &input.path).await?;
    if !fs::metadata(&parent)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?
        .is_dir()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let directory = parent.join(&input.name);
    fs::create_dir(&directory).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;
    let relative = directory
        .strip_prefix(state.media_root.as_path())
        .map_err(|_| StatusCode::FORBIDDEN)?
        .to_string_lossy()
        .replace('\\', "/");
    Ok((
        StatusCode::CREATED,
        Json(CreatedEntry {
            name: input.name,
            path: relative,
        }),
    ))
}

fn validate_entry_name(name: &str) -> Result<(), StatusCode> {
    if name.trim().is_empty()
        || name.len() > 240
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
        || Path::new(name)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

async fn stream_media(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let path = resolve_inside(&state.media_root, &query.path).await?;
    let metadata = fs::metadata(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !metadata.is_file() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let total = metadata.len();
    let (start, end, partial) = match headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        Some(value) => match parse_range(value, total) {
            Some(range) => range,
            None => return Err(StatusCode::RANGE_NOT_SATISFIABLE),
        },
        None => (0, total.saturating_sub(1), false),
    };
    let mut file = File::open(&path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    file.seek(SeekFrom::Start(start))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let length = if total == 0 { 0 } else { end - start + 1 };
    let stream = ReaderStream::with_capacity(file.take(length), 64 * 1024);
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    let mut builder = Response::builder()
        .status(if partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CONTENT_LENGTH, length.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, no-store")
        .header("x-content-type-options", "nosniff");
    if partial {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total}"),
        );
    }
    builder
        .body(Body::from_stream(stream))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn start_download(
    State(state): State<AppState>,
    Extension(user): Extension<SessionUser>,
    Json(input): Json<DownloadInput>,
) -> Result<(StatusCode, Json<DownloadJob>), StatusCode> {
    if input.url.len() > 4096 || Url::parse(&input.url).is_err() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let job = DownloadJob {
        id: id.clone(),
        owner_id: user._id,
        state: "queued".into(),
        bytes: 0,
        total: None,
        saved_as: None,
        error: None,
    };
    {
        let mut jobs = state.downloads.lock().await;
        jobs.retain(|_, job| job.state == "queued" || job.state == "downloading");
        if jobs.len() >= 20 {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        jobs.insert(id.clone(), job.clone());
    }
    let worker_state = state.clone();
    let task_id = id.clone();
    let handle = tokio::spawn(async move {
        let permit = match worker_state.download_slot.clone().acquire_owned().await {
            Ok(value) => value,
            Err(_) => {
                set_job_error(&worker_state, &task_id, "Download worker stopped.").await;
                worker_state.download_tasks.lock().await.remove(&task_id);
                return;
            }
        };
        set_job_state(&worker_state, &task_id, "downloading").await;
        let outcome = download_to_library(&worker_state, &task_id, &input.url).await;
        match outcome {
            Ok((name, bytes)) => {
                if let Some(job) = worker_state.downloads.lock().await.get_mut(&task_id) {
                    job.state = "complete".into();
                    job.bytes = bytes;
                    job.total = Some(bytes);
                    job.saved_as = Some(name);
                }
            }
            Err(message) => set_job_error(&worker_state, &task_id, &message).await,
        }
        worker_state.download_tasks.lock().await.remove(&task_id);
        drop(permit);
    });
    state
        .download_tasks
        .lock()
        .await
        .insert(id.clone(), handle.abort_handle());
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn cancel_download(
    State(state): State<AppState>,
    Extension(user): Extension<SessionUser>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, StatusCode> {
    let mut jobs = state.downloads.lock().await;
    let job = jobs
        .get_mut(&id)
        .filter(|job| job.owner_id == user._id)
        .ok_or(StatusCode::NOT_FOUND)?;

    if job.state != "complete" && job.state != "failed" && job.state != "cancelled" {
        job.state = "cancelled".into();
        job.error = Some("Download cancelled by user.".into());
        drop(jobs);

        if let Some(abort_handle) = state.download_tasks.lock().await.remove(&id) {
            abort_handle.abort();
        }

        let prefix = format!("{}-", &id[..8.min(id.len())]);
        let downloads_dir = state.media_root.join("downloads");
        if let Ok(mut entries) = fs::read_dir(&downloads_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with(&prefix) && name.ends_with(".part") {
                        let _ = fs::remove_file(entry.path()).await;
                    }
                }
            }
        }
    }
    Ok(StatusCode::OK)
}

async fn download_status(
    State(state): State<AppState>,
    Extension(user): Extension<SessionUser>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<DownloadJob>, StatusCode> {
    let jobs = state.downloads.lock().await;
    let job = jobs
        .get(&id)
        .filter(|job| job.owner_id == user._id)
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(job))
}

async fn set_job_state(state: &AppState, id: &str, value: &str) {
    if let Some(job) = state.downloads.lock().await.get_mut(id) {
        job.state = value.into();
    }
}

async fn set_job_error(state: &AppState, id: &str, message: &str) {
    if let Some(job) = state.downloads.lock().await.get_mut(id) {
        job.state = "failed".into();
        job.error = Some(message.into());
    }
}

async fn download_to_library(
    state: &AppState,
    id: &str,
    raw_url: &str,
) -> Result<(String, u64), String> {
    let mut current = Url::parse(raw_url).map_err(|_| "That link is not a valid URL.")?;
    let mut response = None;
    for hop in 0..=5 {
        let addresses = resolve_public_targets(&current).await?;
        let host = current.host_str().ok_or("That link has no hostname.")?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(12))
            .timeout(Duration::from_secs(30 * 60))
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| "Could not initialize the download connection.")?;
        let result = client
            .get(current.clone())
            .send()
            .await
            .map_err(|_| "Could not reach that public download link.")?;
        if result.status().is_redirection() {
            if hop == 5 {
                return Err("The link redirected too many times.".into());
            }
            let location = result
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or("The remote redirect was invalid.")?;
            let next = current
                .join(location)
                .map_err(|_| "The remote redirect was invalid.")?;
            if current.scheme() == "https" && next.scheme() != "https" {
                return Err("The link tried to downgrade from HTTPS.".into());
            }
            current = next;
            continue;
        }
        response = Some(result);
        break;
    }
    let response = response.ok_or("Could not complete the remote request.")?;
    if !response.status().is_success() {
        return Err(format!(
            "The remote server returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    let cap = std::env::var("MABAEIREAM_MAX_DOWNLOAD_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(20 * 1024 * 1024 * 1024);
    let expected = response.content_length();
    if expected.is_some_and(|value| value > cap) {
        return Err("The file is larger than the configured download limit.".into());
    }

    let file_name = safe_file_name(&current);
    let relative_name = format!("downloads/{}-{}", &id[..8], file_name);
    let directory = state.media_root.join("downloads");
    fs::create_dir_all(&directory)
        .await
        .map_err(|_| "Could not prepare the downloads folder.")?;
    let destination = state.media_root.join(&relative_name);
    let partial = state.media_root.join(format!("{relative_name}.part"));
    let _cleanup = RemoveOnDrop(partial.clone());
    let mut file = File::create(&partial)
        .await
        .map_err(|_| "Could not create the download file.")?;
    let mut stream = response.bytes_stream();
    let mut received = 0_u64;
    let mut last_reported = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "The download connection ended unexpectedly.")?;
        received = received.saturating_add(chunk.len() as u64);
        if received > cap {
            return Err("The file is larger than the configured download limit.".into());
        }
        file.write_all(&chunk)
            .await
            .map_err(|_| "Could not write the downloaded file.")?;
        if received - last_reported >= 1024 * 1024 {
            last_reported = received;
            if let Some(job) = state.downloads.lock().await.get_mut(id) {
                job.bytes = received;
                job.total = expected;
            }
        }
    }
    file.flush()
        .await
        .map_err(|_| "Could not finish writing the downloaded file.")?;
    drop(file);
    fs::rename(&partial, &destination)
        .await
        .map_err(|_| "Could not save the completed download.")?;
    Ok((relative_name, received))
}

async fn resolve_public_targets(url: &Url) -> Result<Vec<SocketAddr>, String> {
    if !matches!(url.scheme(), "http" | "https") || url.username() != "" || url.password().is_some()
    {
        return Err("Only public HTTP and HTTPS file links are supported.".into());
    }
    let host = url.host_str().ok_or("That link has no hostname.")?;
    let port = url
        .port_or_known_default()
        .ok_or("That link has an unsupported port.")?;
    if !matches!(port, 80 | 443)
        || host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".local")
    {
        return Err("Only public web addresses on ports 80 and 443 are allowed.".into());
    }
    let addresses = if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| "The link hostname could not be resolved.")?
            .collect::<Vec<_>>()
    };
    if addresses.is_empty() || addresses.iter().any(|addr| !is_public_ip(addr.ip())) {
        return Err("The link must resolve only to public internet addresses.".into());
    }
    Ok(addresses)
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => {
            let n = u32::from(value);
            let in_range = |network: Ipv4Addr, prefix: u32| {
                let mask = if prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - prefix)
                };
                n & mask == u32::from(network) & mask
            };
            !(value.is_private()
                || value.is_loopback()
                || value.is_link_local()
                || value.is_broadcast()
                || value.is_multicast()
                || value.is_unspecified()
                || in_range(Ipv4Addr::new(0, 0, 0, 0), 8)
                || in_range(Ipv4Addr::new(100, 64, 0, 0), 10)
                || in_range(Ipv4Addr::new(192, 0, 0, 0), 24)
                || in_range(Ipv4Addr::new(192, 0, 2, 0), 24)
                || in_range(Ipv4Addr::new(192, 88, 99, 0), 24)
                || in_range(Ipv4Addr::new(198, 18, 0, 0), 15)
                || in_range(Ipv4Addr::new(198, 51, 100, 0), 24)
                || in_range(Ipv4Addr::new(203, 0, 113, 0), 24)
                || in_range(Ipv4Addr::new(240, 0, 0, 0), 4))
        }
        IpAddr::V6(value) => {
            if let Some(mapped) = value.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(mapped));
            }
            let segments = value.segments();
            let global_unicast = segments[0] & 0xe000 == 0x2000;
            let documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
            let special_2001 = segments[0] == 0x2001 && segments[1] <= 0x01ff;
            global_unicast
                && !documentation
                && !special_2001
                && segments[0] != 0x2002
                && !value.is_loopback()
                && !value.is_unspecified()
                && !value.is_multicast()
                && !value.is_unique_local()
        }
    }
}

fn safe_file_name(url: &Url) -> String {
    let candidate = url
        .path_segments()
        .and_then(|mut parts| parts.next_back())
        .unwrap_or("");
    let cleaned: String = candidate
        .chars()
        .take(96)
        .filter(|value| value.is_ascii_alphanumeric() || matches!(*value, '.' | '-' | '_'))
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "download.bin".into()
    } else {
        cleaned
    }
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn resolve_inside(root: &Path, relative: &str) -> Result<PathBuf, StatusCode> {
    let requested = Path::new(relative);
    if requested.is_absolute()
        || requested
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let canonical = fs::canonicalize(root.join(requested))
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !canonical.starts_with(root) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(canonical)
}

fn parse_range(value: &str, total: u64) -> Option<(u64, u64, bool)> {
    if total == 0 {
        return None;
    }
    let range = value.strip_prefix("bytes=")?;
    if range.contains(',') {
        return None;
    }
    let (start, end) = range.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?.min(total);
        if suffix == 0 {
            return None;
        }
        return Some((total - suffix, total - 1, true));
    }
    let start = start.parse::<u64>().ok()?;
    if start >= total {
        return None;
    }
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<u64>().ok()?.min(total - 1)
    };
    (end >= start).then_some((start, end, true))
}

fn is_video(extension: Option<&str>) -> bool {
    matches!(
        extension.unwrap_or_default().to_ascii_lowercase().as_str(),
        "mp4" | "m4v" | "mkv" | "webm" | "mov" | "avi" | "mpeg" | "mpg" | "m3u8" | "mpd"
    )
}

fn hex_digest(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod media_resolution_tests {
    use super::{select_resolved_streams, validate_resolvable_provider_url};
    use axum::http::StatusCode;

    #[test]
    fn resolver_accepts_only_known_video_provider_https_urls() {
        assert!(validate_resolvable_provider_url("https://www.youtube.com/watch?v=abc").is_ok());
        assert!(validate_resolvable_provider_url("https://youtu.be/abc").is_ok());
        assert!(validate_resolvable_provider_url("https://player.vimeo.com/video/123").is_ok());
        for rejected in [
            "http://youtube.com/watch?v=abc",
            "https://youtube.com.attacker.test/watch?v=abc",
            "https://127.0.0.1/private",
            "https://youtube.com:8443/watch?v=abc",
            "https://user:pass@youtube.com/watch?v=abc",
        ] {
            assert_eq!(
                validate_resolvable_provider_url(rejected).unwrap_err(),
                StatusCode::BAD_REQUEST,
            );
        }
    }

    #[test]
    fn resolver_preserves_separate_video_and_audio_tracks() {
        let metadata = serde_json::json!({
            "requested_formats": [
                {"url": "https://cdn.example/video", "vcodec": "avc1", "acodec": "none"},
                {"url": "https://cdn.example/audio", "vcodec": "none", "acodec": "mp4a"}
            ]
        });
        assert_eq!(
            select_resolved_streams(&metadata).unwrap(),
            (
                "https://cdn.example/video".to_owned(),
                Some("https://cdn.example/audio".to_owned()),
            )
        );
    }

    #[test]
    fn resolver_rejects_non_https_track_urls() {
        let metadata = serde_json::json!({
            "requested_formats": [
                {"url": "http://cdn.example/video", "vcodec": "avc1", "acodec": "none"},
                {"url": "https://cdn.example/audio", "vcodec": "none", "acodec": "mp4a"}
            ]
        });
        assert_eq!(
            select_resolved_streams(&metadata).unwrap_err(),
            StatusCode::BAD_GATEWAY,
        );
    }
}

#[cfg(test)]
mod media_upload_tests {
    use super::{is_safe_live_hls_path, validate_entry_name};
    use axum::http::StatusCode;

    #[test]
    fn upload_names_allow_normal_unicode_names_and_reject_paths() {
        assert!(validate_entry_name("A quiet evening.mp4").is_ok());
        assert!(validate_entry_name("映像.mov").is_ok());
        for rejected in [
            "",
            " ",
            ".",
            "..",
            "../movie.mp4",
            "folder\\movie.mp4",
            "C:movie.mp4",
        ] {
            assert_eq!(
                validate_entry_name(rejected).unwrap_err(),
                StatusCode::BAD_REQUEST
            );
        }
    }

    #[test]
    fn live_hls_proxy_accepts_only_the_configured_stream_path() {
        let stream = "mabaeiream/a1b2c3d4e5f6g7h8";
        assert!(is_safe_live_hls_path(
            "mabaeiream/a1b2c3d4e5f6g7h8/index.m3u8",
            stream,
        ));
        for rejected in [
            "mabaeiream/other/index.m3u8",
            "mabaeiream/a1b2c3d4e5f6g7h8/../secret",
            "mabaeiream/a1b2c3d4e5f6g7h8/%2e%2e/secret",
            "mabaeiream/a1b2c3d4e5f6g7h8/",
        ] {
            assert!(!is_safe_live_hls_path(rejected, stream));
        }
    }
}

#[cfg(test)]
mod room_tests {
    use super::*;

    #[test]
    fn https_sources_must_be_secure_and_credential_free() {
        assert!(validate_https_source("https://media.example/video.mp4").is_ok());
        assert!(validate_https_source("http://media.example/video.mp4").is_err());
        assert!(validate_https_source("https://alice:secret@media.example/video.mp4").is_err());
        assert!(validate_https_source("https://@media.example/video.mp4").is_err());
        assert!(validate_https_source("https://").is_err());
    }

    #[tokio::test]
    async fn library_sources_must_be_relative_video_files_inside_media_root() {
        let root = std::env::temp_dir().join(format!("mabaeiream-room-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).await.unwrap();
        fs::write(root.join("movie.mp4"), b"video").await.unwrap();
        fs::write(root.join("notes.txt"), b"text").await.unwrap();
        let canonical_root = fs::canonicalize(&root).await.unwrap();

        let good = validate_room_command(
            &canonical_root,
            RoomCommand::Select {
                source_kind: "library".into(),
                source: "movie.mp4".into(),
                title: "Movie".into(),
                playing: true,
            },
        )
        .await;
        assert!(matches!(
            good,
            Ok(ValidatedRoomCommand::Select { playing: true, .. })
        ));

        for source in [
            "../movie.mp4",
            "notes.txt",
            "missing.mp4",
            "movie\\clip.mp4",
        ] {
            let result = validate_room_command(
                &canonical_root,
                RoomCommand::Select {
                    source_kind: "library".into(),
                    source: source.into(),
                    title: "Movie".into(),
                    playing: true,
                },
            )
            .await;
            assert!(
                result.is_err(),
                "unexpectedly accepted library path {source}"
            );
        }
        fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn room_state_changes_have_monotonic_revisions_and_server_anchored_position() {
        let mut room = RoomState::default();
        let source = RoomSource {
            kind: "https",
            source: "https://media.example/movie.mp4".into(),
            title: "Movie".into(),
        };
        room.apply(
            ValidatedRoomCommand::Select {
                source,
                playing: false,
            },
            "amir",
            1_000,
        )
        .unwrap();
        assert_eq!(room.revision, 1);
        room.apply(ValidatedRoomCommand::Play(500), "partner", 2_000)
            .unwrap();
        assert_eq!(room.revision, 2);
        assert!(room.playing);
        assert_eq!(room.current_position_ms(3_000), 1_500);

        room.apply(ValidatedRoomCommand::Seek(100), "amir", 4_000)
            .unwrap();
        assert_eq!(room.revision, 3);
        assert_eq!(room.current_position_ms(5_000), 1_100);
        room.apply(ValidatedRoomCommand::Pause(1_200), "partner", 8_000)
            .unwrap();
        assert_eq!(room.revision, 4);
        assert_eq!(room.current_position_ms(9_000), 1_200);

        let message = room.state_message(9_000);
        assert_eq!(message["type"], "state");
        assert_eq!(message["revision"], 4);
        assert_eq!(message["source_kind"], "https");
        assert_eq!(message["source"], "https://media.example/movie.mp4");
        assert_eq!(message["title"], "Movie");
        assert_eq!(message["playing"], false);
        assert_eq!(message["updated_by"], "partner");
    }

    #[test]
    fn room_commands_cannot_spoof_or_supply_unrecognized_fields() {
        let command =
            serde_json::from_str::<RoomCommand>(r#"{"type":"play","position_ms":10,"user_id":1}"#);
        assert!(command.is_err());
    }

    #[test]
    fn select_can_start_playback_in_the_same_room_revision() {
        let command = serde_json::from_str::<RoomCommand>(
            r#"{"type":"select","source_kind":"https","source":"https://media.example/movie.mp4","title":"Movie","playing":true}"#,
        )
        .unwrap();
        let RoomCommand::Select { playing, .. } = command else {
            panic!("expected a media selection");
        };
        assert!(playing);

        let mut room = RoomState::default();
        room.apply(
            ValidatedRoomCommand::Select {
                source: RoomSource {
                    kind: "https",
                    source: "https://media.example/movie.mp4".into(),
                    title: "Movie".into(),
                },
                playing,
            },
            "amir",
            1_000,
        )
        .unwrap();
        let state = room.state_message(1_000);
        assert_eq!(state["revision"], 1);
        assert_eq!(state["playing"], true);
    }

    #[test]
    fn older_select_commands_default_to_paused() {
        let command = serde_json::from_str::<RoomCommand>(
            r#"{"type":"select","source_kind":"https","source":"https://media.example/movie.mp4","title":"Movie"}"#,
        )
        .unwrap();
        let RoomCommand::Select { playing, .. } = command else {
            panic!("expected a media selection");
        };
        assert!(!playing);
    }

    #[test]
    fn signaling_accepts_only_the_webrtc_offer_answer_and_end_shapes() {
        assert!(matches!(
            parse_room_message(r#"{"type":"signal","kind":"offer","sdp":"v=0\r\n..."}"#),
            Ok(RoomMessage::Signal(ValidatedSignal::Offer { .. }))
        ));
        assert!(matches!(
            parse_room_message(r#"{"type":"signal","kind":"answer","sdp":"v=0\r\n..."}"#),
            Ok(RoomMessage::Signal(ValidatedSignal::Answer { .. }))
        ));
        assert!(matches!(
            parse_room_message(
                r#"{"type":"signal","kind":"candidate","candidate":"candidate:1 1 udp 2122260223 192.0.2.1 5000 typ host","sdp_mid":"audio","sdp_mline_index":0}"#
            ),
            Ok(RoomMessage::Signal(ValidatedSignal::Candidate { .. }))
        ));
        assert!(matches!(
            parse_room_message(r#"{"type":"signal","kind":"end"}"#),
            Ok(RoomMessage::Signal(ValidatedSignal::End))
        ));

        for invalid in [
            r#"{"type":"signal","kind":"offer","sdp":"v=0\r\n...","from":"attacker"}"#,
            r#"{"type":"signal","kind":"offer"}"#,
            r#"{"type":"signal","kind":"offer","sdp":"invalid"}"#,
            r#"{"type":"signal","kind":"candidate","candidate":"bogus"}"#,
            r#"{"type":"signal","kind":"candidate","candidate":"candidate:1","unexpected":true}"#,
            r#"{"type":"signal","kind":"end","ticket":"unexpected"}"#,
            r#"{"type":"signal","kind":"unknown"}"#,
            r#"{"type":"signal","type":"play","kind":"offer","sdp":"v=0"}"#,
        ] {
            match parse_room_message(invalid) {
                Err(_) => {}
                Ok(RoomMessage::Signal(signal)) => {
                    assert!(validate_signal(&signal).is_err(), "accepted {invalid}");
                }
                Ok(_) => panic!("accepted {invalid}"),
            }
        }
    }

    #[test]
    fn webrtc_session_descriptions_are_bounded_and_well_formed() {
        let accepted = format!("v=0{}", "a".repeat(VOICE_MAX_SDP_BYTES - 3));
        assert!(validate_signal(&ValidatedSignal::Offer { sdp: accepted }).is_ok());

        let oversized = format!("v=0{}", "a".repeat(VOICE_MAX_SDP_BYTES - 2));
        assert!(validate_signal(&ValidatedSignal::Answer { sdp: oversized }).is_err());
        assert!(
            validate_signal(&ValidatedSignal::Offer {
                sdp: "v=0\u{1}bad".into()
            })
            .is_err()
        );
    }

    #[test]
    fn webrtc_ice_candidates_are_bounded_and_well_formed() {
        let valid = ValidatedSignal::Candidate {
            candidate: "candidate:1 1 udp 2122260223 192.0.2.1 5000 typ host".into(),
            sdp_mid: Some("audio".into()),
            sdp_mline_index: Some(0),
        };
        assert!(validate_signal(&valid).is_ok());
        assert!(
            validate_signal(&ValidatedSignal::Candidate {
                candidate: format!("candidate:{}", "x".repeat(2048)),
                sdp_mid: None,
                sdp_mline_index: None,
            })
            .is_err()
        );
    }

    #[test]
    fn server_signal_shape_uses_authenticated_sender_name() {
        let outgoing = AuthenticatedSignal {
            sender_id: 7,
            sender_session_hash: "session-a".into(),
            from: "alice".into(),
            signal: ValidatedSignal::Offer {
                sdp: "v=0\r\n...".into(),
            },
        }
        .message();
        assert_eq!(outgoing["type"], "signal");
        assert_eq!(outgoing["kind"], "offer");
        assert_eq!(outgoing["from"], "alice");
        assert_eq!(outgoing["sdp"], "v=0\r\n...");
    }

    #[test]
    fn signaling_reaches_other_sessions_but_not_the_sending_session() {
        let signal = AuthenticatedSignal {
            sender_id: 7,
            sender_session_hash: "session-a".into(),
            from: "alice".into(),
            signal: ValidatedSignal::End,
        };
        assert!(!signal.should_deliver_to(7, "session-a"));
        assert!(signal.should_deliver_to(7, "session-b"));
        assert!(signal.should_deliver_to(8, "session-c"));
    }

    #[test]
    fn turn_credentials_are_expiring_user_scoped_hmac_values() {
        let servers = make_turn_servers(
            "turn:ahura.site:3478?transport=udp, turns:ahura.site:5349?transport=tcp",
            Some("test-shared-turn-secret-that-is-long-enough"),
            42,
            1_800_000_000,
        )
        .unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(
            servers[0].urls,
            [
                "turn:ahura.site:3478?transport=udp",
                "turns:ahura.site:5349?transport=tcp"
            ]
        );
        assert_eq!(servers[0].username.as_deref(), Some("1800000000:42"));

        let mut mac =
            Hmac::<Sha1>::new_from_slice(b"test-shared-turn-secret-that-is-long-enough").unwrap();
        mac.update(b"1800000000:42");
        let expected =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        assert_eq!(servers[0].credential.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn turn_configuration_rejects_missing_secrets_and_non_turn_urls() {
        assert!(make_turn_servers("turn:ahura.site:3478", None, 1, 100).is_err());
        assert!(
            make_turn_servers(
                "stun:attacker.invalid:3478",
                Some("test-shared-turn-secret-that-is-long-enough"),
                1,
                100,
            )
            .is_err()
        );
        assert!(make_turn_servers("", None, 1, 100).unwrap().is_empty());
    }
}

#[cfg(test)]
mod subtitle_sidecar_tests {
    use super::{is_sidecar_for, subtitle_mime_type};

    #[test]
    fn subtitle_sidecars_match_video_stems_and_supported_formats() {
        assert!(is_sidecar_for("Movie.Final", "movie.final.en.srt"));
        assert!(is_sidecar_for("Movie", "movie.vtt"));
        assert!(!is_sidecar_for("Movie", "movie-trailer.srt"));
        assert!(!is_sidecar_for("Movie", "other.srt"));
        assert_eq!(subtitle_mime_type("SRT"), Some("application/x-subrip"));
        assert_eq!(subtitle_mime_type("vtt"), Some("text/vtt"));
        assert_eq!(subtitle_mime_type("txt"), None);
    }
}
