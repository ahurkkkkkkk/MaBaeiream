slint::include_modules!();
mod voice;

use serde::Deserialize;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::{
    env,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{
    Message, WebSocket, client::IntoClientRequest, http::HeaderValue, stream::MaybeTlsStream,
};
use ureq::SendBody;
use url::Url;

static NEXT_IPC_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
struct Session {
    base: String,
    token: String,
    room: Sender<RoomOutbound>,
    voice: Sender<voice::Command>,
}

enum RoomOutbound {
    Message(serde_json::Value),
    Stop,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum RoomMessage {
    #[serde(rename = "state")]
    State {
        revision: u64,
        source_kind: Option<String>,
        source: Option<String>,
        title: Option<String>,
        playing: bool,
        position_ms: u64,
        updated_by: Option<String>,
    },
    #[serde(rename = "signal")]
    Signal {
        kind: String,
        from: String,
        sdp: Option<String>,
        candidate: Option<String>,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

struct Playback {
    key: String,
    revision: u64,
    endpoint: String,
    child: std::process::Child,
    playing: bool,
    position_ms: u64,
    room_position_ms: u64,
    room_anchor: Instant,
    room_playing: bool,
    sync_speed: f64,
    suppress_local_until: Instant,
    is_live: bool,
}

type SharedPlayback = Arc<Mutex<Option<Playback>>>;

#[derive(Deserialize)]
struct LoginReply {
    token: String,
    username: String,
}

#[derive(Deserialize)]
struct LibraryReply {
    items: Vec<ApiItem>,
}

#[derive(Deserialize)]
struct ApiItem {
    name: String,
    path: String,
    kind: String,
    size: u64,
}

#[derive(Deserialize)]
struct SubtitleReply {
    subtitles: Vec<SubtitleItem>,
}

#[derive(Deserialize)]
struct SubtitleItem {
    path: String,
}

#[derive(Deserialize)]
struct DownloadReply {
    id: String,
    state: String,
    bytes: u64,
    total: Option<u64>,
    saved_as: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Deserialize)]
struct LiveConfig {
    stream_path: String,
    whip_url: String,
    rtmps_server_url: String,
    stream_key: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let window = MainWindow::new()?;
    let session: Arc<Mutex<Option<Session>>> = Arc::new(Mutex::new(None));
    let current_folder = Arc::new(Mutex::new(String::new()));
    let active_download: Arc<Mutex<Option<(Session, String)>>> = Arc::new(Mutex::new(None));
    let playback: SharedPlayback = Arc::new(Mutex::new(None));
    let weak = window.as_weak();

    window.on_connect_requested({
        let session = session.clone();
        let folder = current_folder.clone();
        let playback = playback.clone();
        let weak = weak.clone();
        move |server, username, password| {
            set_status(&weak, "Connecting securely…");
            let session = session.clone();
            let folder = folder.clone();
            let playback = playback.clone();
            let weak = weak.clone();
            let base = server.to_string().trim_end_matches('/').to_owned();
            let username = username.to_string();
            let password = password.to_string();
            thread::spawn(move || {
                let result = connect(&base, &username, &password);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        match result {
                            Ok((token, account, items, live)) => {
                                let (room, voice) = start_room_connection(
                                    base.clone(),
                                    token.clone(),
                                    weak.clone(),
                                    playback.clone(),
                                );
                                *session.lock().expect("session lock poisoned") = Some(Session {
                                    base,
                                    token,
                                    room,
                                    voice,
                                });
                                *folder.lock().expect("folder lock poisoned") = String::new();
                                ui.set_folder_path(SharedString::default());
                                ui.set_folder_name(SharedString::default());
                                ui.set_file_query(SharedString::default());
                                ui.set_username(account.into());
                                ui.set_password(SharedString::default());
                                ui.set_connected(true);
                                ui.set_is_downloading(false);
                                ui.set_media_items(to_model(items));
                                if let Some(config) = live {
                                    ui.set_live_stream_path(config.stream_path.into());
                                    ui.set_live_whip_url(config.whip_url.into());
                                    ui.set_live_rtmps_url(config.rtmps_server_url.into());
                                    ui.set_live_stream_key(config.stream_key.into());
                                } else {
                                    ui.set_live_stream_path(SharedString::default());
                                    ui.set_live_whip_url(SharedString::default());
                                    ui.set_live_rtmps_url(SharedString::default());
                                    ui.set_live_stream_key(SharedString::default());
                                }
                                ui.set_status_text("Joining watch room…".into());
                            }
                            Err(message) => ui.set_status_text(message.into()),
                        }
                    }
                });
            });
        }
    });

    window.on_refresh_requested({
        let session = session.clone();
        let folder = current_folder.clone();
        let weak = weak.clone();
        move || refresh(&session, &folder, &weak)
    });

    window.on_folder_up_requested({
        let session = session.clone();
        let folder = current_folder.clone();
        let weak = weak.clone();
        move || {
            let parent = {
                let mut current = folder.lock().expect("folder lock poisoned");
                *current = current
                    .rsplit_once('/')
                    .map(|(parent, _)| parent.to_owned())
                    .unwrap_or_default();
                current.clone()
            };
            if let Some(ui) = weak.upgrade() {
                ui.set_folder_path(parent.clone().into());
            }
            refresh(&session, &folder, &weak);
        }
    });

    window.on_create_folder_requested({
        let session = session.clone();
        let folder = current_folder.clone();
        let weak = weak.clone();
        move |name| {
            let Some(active) = session.lock().expect("session lock poisoned").clone() else {
                set_status(&weak, "Connect to your server before creating a folder.");
                return;
            };
            let name = name.to_string().trim().to_owned();
            if name.is_empty() {
                set_status(&weak, "Enter a name for the new folder.");
                return;
            }
            let destination = folder.lock().expect("folder lock poisoned").clone();
            set_status(&weak, "Creating folder…");
            let weak = weak.clone();
            thread::spawn(move || {
                let result = ureq::post(&format!("{}/api/v1/folders", active.base))
                    .header("Authorization", &format!("Bearer {}", active.token))
                    .send_json(serde_json::json!({"name": name, "path": destination}))
                    .map(|_| ())
                    .map_err(|error| error.to_string())
                    .and_then(|()| list_items(&active.base, &active.token, &destination));
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        match result {
                            Ok(items) => {
                                ui.set_media_items(to_model(items));
                                ui.set_folder_name(SharedString::default());
                                ui.set_status_text("Folder created.".into());
                            }
                            Err(error) => ui.set_status_text(
                                format!("Could not create folder: {error}").into(),
                            ),
                        }
                    }
                });
            });
        }
    });

    window.on_sign_out_requested({
        let session = session.clone();
        let active_download = active_download.clone();
        let playback = playback.clone();
        let weak = weak.clone();
        move || {
            *active_download.lock().expect("download lock poisoned") = None;
            if let Some(current) = session.lock().expect("session lock poisoned").take() {
                let _ = current.voice.send(voice::Command::Stop);
                let _ = current.room.send(RoomOutbound::Stop);
                thread::spawn(move || {
                    let _ = post_logout(&current);
                });
            }
            stop_playback(&playback);
            if let Some(ui) = weak.upgrade() {
                ui.set_connected(false);
                ui.set_folder_path(SharedString::default());
                ui.set_folder_name(SharedString::default());
                ui.set_file_query(SharedString::default());
                ui.set_is_downloading(false);
                ui.set_media_items(to_model(Vec::new()));
                ui.set_live_stream_path(SharedString::default());
                ui.set_live_whip_url(SharedString::default());
                ui.set_live_rtmps_url(SharedString::default());
                ui.set_live_stream_key(SharedString::default());
                ui.set_status_text(SharedString::default());
            }
        }
    });

    window.on_voice_call_requested({
        let session = session.clone();
        let weak = weak.clone();
        move || {
            if let Some(active) = session.lock().expect("session lock poisoned").clone() {
                let _ = active.voice.send(voice::Command::Call);
            } else {
                set_status(
                    &weak,
                    "Connect to your server before starting a voice call.",
                );
            }
        }
    });
    window.on_voice_answer_requested({
        let session = session.clone();
        move || {
            if let Some(active) = session.lock().expect("session lock poisoned").clone() {
                let _ = active.voice.send(voice::Command::Accept);
            }
        }
    });
    window.on_voice_decline_requested({
        let session = session.clone();
        move || {
            if let Some(active) = session.lock().expect("session lock poisoned").clone() {
                let _ = active.voice.send(voice::Command::Decline);
            }
        }
    });
    window.on_voice_end_requested({
        let session = session.clone();
        move || {
            if let Some(active) = session.lock().expect("session lock poisoned").clone() {
                let _ = active.voice.send(voice::Command::End);
            }
        }
    });

    window.on_media_open_requested({
        let session = session.clone();
        let current_folder = current_folder.clone();
        let weak = weak.clone();
        move |path, name, kind| {
            if kind.as_str() == "folder" {
                *current_folder.lock().expect("folder lock poisoned") = path.to_string();
                if let Some(ui) = weak.upgrade() {
                    ui.set_folder_path(path.clone());
                    ui.set_file_query(SharedString::default());
                }
                refresh(&session, &current_folder, &weak);
            } else if kind.as_str() == "video" {
                let active = session.lock().expect("session lock poisoned").clone();
                if let Some(active) = active {
                    send_room_selection(&active, "library", path.as_str(), name.as_str(), &weak);
                }
            }
        }
    });

    window.on_upload_requested({
        let session = session.clone();
        let folder = current_folder.clone();
        let weak = weak.clone();
        move || {
            let Some(active) = session.lock().expect("session lock poisoned").clone() else {
                set_status(&weak, "Connect to your server before uploading files.");
                return;
            };
            let destination = folder.lock().expect("folder lock poisoned").clone();
            let weak = weak.clone();
            let session_state = session.clone();
            let folder_state = folder.clone();
            thread::spawn(move || {
                let selected = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| error.to_string())
                    .map(|runtime| {
                        runtime.block_on(
                            rfd::AsyncFileDialog::new()
                                .set_title("Add files to your MaBaeiream library")
                                .pick_files(),
                        )
                    });
                let paths = match selected {
                    Ok(Some(files)) => files
                        .into_iter()
                        .map(|file| file.path().to_owned())
                        .collect::<Vec<_>>(),
                    Ok(None) => {
                        set_status(&weak, "No files selected.");
                        return;
                    }
                    Err(error) => {
                        set_status(&weak, &format!("Could not open the file picker: {error}"));
                        return;
                    }
                };
                if paths.is_empty() {
                    set_status(&weak, "No files selected.");
                    return;
                }
                let total = paths.len();
                for (index, path) in paths.iter().enumerate() {
                    let name = path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("file");
                    set_status(
                        &weak,
                        &format!("Uploading {} ({}/{})…", name, index + 1, total),
                    );
                    if let Err(error) = upload_local_file(&active, path, &destination) {
                        set_status(&weak, &format!("Upload failed for {name}: {error}"));
                        return;
                    }
                }
                set_status(
                    &weak,
                    &format!("Uploaded {total} file(s) to your shared library."),
                );
                let _ = slint::invoke_from_event_loop({
                    let weak = weak.clone();
                    move || refresh(&session_state, &folder_state, &weak)
                });
            });
        }
    });

    window.on_watch_live_requested({
        let session = session.clone();
        let weak = weak.clone();
        move || {
            let Some(active) = session.lock().expect("session lock poisoned").clone() else {
                set_status(
                    &weak,
                    "Connect to your server before watching the desktop stream.",
                );
                return;
            };
            let path = weak
                .upgrade()
                .map(|ui| ui.get_live_stream_path().to_string())
                .unwrap_or_default();
            if path.is_empty() {
                set_status(&weak, "Desktop streaming is not set up on this server yet.");
                return;
            }
            send_room_selection(&active, "live", &path, "Live from desktop", &weak);
        }
    });

    window.on_play_link_requested({
        let session = session.clone();
        let weak = weak.clone();
        move |link| {
            let Some(active) = session.lock().expect("session lock poisoned").clone() else {
                set_status(&weak, "Connect to your server before playing a link.");
                return;
            };
            let value = link.to_string();
            let title = Url::parse(&value)
                .ok()
                .map(|url| {
                    url.path_segments()
                        .and_then(|mut segments| segments.next_back())
                        .filter(|segment| !segment.is_empty())
                        .unwrap_or_else(|| url.host_str().unwrap_or("Video"))
                        .to_owned()
                })
                .unwrap_or_else(|| "Video link".into());
            send_room_selection(&active, "https", &value, &title, &weak);
        }
    });

    window.on_download_link_requested({
        let session = session.clone();
        let folder = current_folder.clone();
        let active_download = active_download.clone();
        let weak = weak.clone();
        move |link| {
            let Some(active) = session.lock().expect("session lock poisoned").clone() else {
                return;
            };
            let value = link.to_string();
            let weak = weak.clone();
            let session = session.clone();
            let folder = folder.clone();
            let active_download = active_download.clone();
            thread::spawn(move || {
                let result =
                    validate_download_url(&value).and_then(|_| queue_download(&active, &value));
                match result {
                    Ok(id) => {
                        *active_download.lock().expect("download lock poisoned") =
                            Some((active.clone(), id.clone()));
                        let _ = slint::invoke_from_event_loop({
                            let weak = weak.clone();
                            move || {
                                if let Some(ui) = weak.upgrade() {
                                    ui.set_is_downloading(true);
                                }
                                set_status(&weak, "Download queued…");
                            }
                        });
                        let outcome = poll_download(&active, &id, &weak);
                        let _ = slint::invoke_from_event_loop({
                            let weak = weak.clone();
                            move || {
                                if let Some(ui) = weak.upgrade() {
                                    ui.set_is_downloading(false);
                                }
                            }
                        });
                        *active_download.lock().expect("download lock poisoned") = None;
                        if outcome.as_deref() == Some("complete") {
                            let _ = slint::invoke_from_event_loop({
                                let session = session.clone();
                                let folder = folder.clone();
                                let weak = weak.clone();
                                move || refresh(&session, &folder, &weak)
                            });
                        }
                    }
                    Err(message) => set_status(&weak, &message),
                }
            });
        }
    });

    window.on_cancel_download_requested({
        let active_download = active_download.clone();
        let weak = weak.clone();
        move || {
            if let Some((session, id)) = active_download
                .lock()
                .expect("download lock poisoned")
                .take()
            {
                set_status(&weak, "Cancelling download…");
                if let Some(ui) = weak.upgrade() {
                    ui.set_is_downloading(false);
                }
                let weak = weak.clone();
                thread::spawn(move || {
                    let _ = cancel_download_job(&session, &id);
                    set_status(&weak, "Download cancelled.");
                });
            }
        }
    });

    window.run()?;
    Ok(())
}

fn connect(
    base: &str,
    username: &str,
    password: &str,
) -> Result<(String, String, Vec<ApiItem>, Option<LiveConfig>), String> {
    validate_https(base)?;
    let mut response = ureq::post(&format!("{base}/api/v1/auth/login"))
        .send_json(serde_json::json!({"username": username, "password": password}))
        .map_err(|error| error.to_string())?;
    let login: LoginReply = response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())?;
    let items = list_items(base, &login.token, "")?;
    let live = fetch_live_config(base, &login.token).ok();
    Ok((login.token, login.username, items, live))
}

fn fetch_live_config(base: &str, token: &str) -> Result<LiveConfig, String> {
    let mut response = ureq::get(&format!("{base}/api/v1/live/config"))
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|error| error.to_string())?;
    response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())
}

fn upload_local_file(session: &Session, file_path: &Path, destination: &str) -> Result<(), String> {
    let name = file_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "The selected filename is not valid UTF-8.".to_owned())?;
    let mut address = Url::parse(&format!("{}/api/v1/uploads", session.base))
        .map_err(|error| error.to_string())?;
    address
        .query_pairs_mut()
        .append_pair("path", destination)
        .append_pair("name", name);
    let mut file = std::fs::File::open(file_path).map_err(|error| error.to_string())?;
    let size = file.metadata().map_err(|error| error.to_string())?.len();
    let response = ureq::post(address.as_str())
        .header("Authorization", &format!("Bearer {}", session.token))
        .header("Content-Type", "application/octet-stream")
        .header("Content-Length", &size.to_string())
        .send(SendBody::from_reader(&mut file))
        .map_err(|error| error.to_string())?;
    if response.status().as_u16() != 201 {
        return Err(format!(
            "server returned HTTP {}",
            response.status().as_u16()
        ));
    }
    Ok(())
}

fn start_room_connection(
    base: String,
    token: String,
    weak: slint::Weak<MainWindow>,
    playback: SharedPlayback,
) -> (Sender<RoomOutbound>, Sender<voice::Command>) {
    let (sender, receiver) = mpsc::channel();
    let voice = voice::spawn(sender.clone(), weak.clone(), base.clone(), token.clone());
    let worker_sender = sender.clone();
    let voice_worker = voice.clone();
    thread::spawn(move || {
        room_connection_loop(
            base,
            token,
            receiver,
            worker_sender,
            weak,
            playback,
            voice_worker,
        )
    });
    (sender, voice)
}

fn send_room_selection(
    session: &Session,
    source_kind: &str,
    source: &str,
    title: &str,
    weak: &slint::Weak<MainWindow>,
) {
    if source_kind == "https" {
        if let Err(message) = validate_https(source) {
            set_status(weak, &message);
            return;
        }
    } else if source_kind == "live" {
        if let Some(ui) = weak.upgrade()
            && ui.get_live_stream_path().as_str() != source
        {
            set_status(weak, "Desktop streaming is not configured for this room.");
            return;
        }
    } else if source_kind != "library" || source.is_empty() {
        set_status(weak, "That library video could not be selected.");
        return;
    }

    let selected = session.room.send(RoomOutbound::Message(serde_json::json!({
        "type": "select",
        "source_kind": source_kind,
        "source": source,
        "title": title,
        "playing": true,
    })));
    if selected.is_err() {
        set_status(weak, "Watch room is reconnecting. Try again in a moment.");
    } else {
        set_status(weak, "Starting shared playback…");
    }
}

fn room_connection_loop(
    base: String,
    token: String,
    receiver: Receiver<RoomOutbound>,
    room_sender: Sender<RoomOutbound>,
    weak: slint::Weak<MainWindow>,
    playback: SharedPlayback,
    voice: Sender<voice::Command>,
) {
    let mut pending = Vec::new();
    let mut retry_delay = Duration::from_secs(1);
    loop {
        match drain_outbound(&receiver, &mut pending) {
            Ok(true) => return,
            Ok(false) => {}
            Err(()) => return,
        }

        let mut socket = match connect_room_socket(&base, &token) {
            Ok(socket) => socket,
            Err(error) => {
                set_status(
                    &weak,
                    &format!("Watch room unavailable ({error}); reconnecting…"),
                );
                if wait_for_room_retry(&receiver, &mut pending, retry_delay) {
                    return;
                }
                retry_delay = (retry_delay * 2).min(Duration::from_secs(15));
                continue;
            }
        };
        retry_delay = Duration::from_secs(1);
        set_status(&weak, "Watch room connected · voice chat ready.");
        set_socket_read_timeout(&mut socket, Duration::from_millis(300));

        let mut disconnected = false;
        while !disconnected {
            match drain_outbound(&receiver, &mut pending) {
                Ok(true) => {
                    let _ = socket.close(None);
                    return;
                }
                Ok(false) => {}
                Err(()) => return,
            }

            if !pending.is_empty() {
                let messages = std::mem::take(&mut pending);
                let mut messages = messages.into_iter();
                while let Some(message) = messages.next() {
                    if let Err(error) = socket.send(Message::Text(message.to_string().into())) {
                        pending.push(message);
                        pending.extend(messages);
                        disconnected = true;
                        set_status(&weak, &format!("Watch room reconnecting… ({error})"));
                        break;
                    }
                }
                if disconnected {
                    break;
                }
            }

            match socket.read() {
                Ok(Message::Text(value)) => {
                    if let Ok(message) = serde_json::from_str::<RoomMessage>(value.as_str()) {
                        match message {
                            RoomMessage::State {
                                revision,
                                source_kind,
                                source,
                                title,
                                playing,
                                position_ms,
                                updated_by,
                            } => {
                                let state = RoomSnapshot {
                                    revision,
                                    source_kind,
                                    source,
                                    title,
                                    playing,
                                    position_ms,
                                    updated_by,
                                };
                                apply_room_state(
                                    &Session {
                                        base: base.clone(),
                                        token: token.clone(),
                                        room: room_sender.clone(),
                                        voice: voice.clone(),
                                    },
                                    &state,
                                    &playback,
                                    &weak,
                                );
                            }
                            RoomMessage::Signal {
                                kind,
                                from,
                                sdp,
                                candidate,
                                sdp_mid,
                                sdp_mline_index,
                            } => {
                                let remote = match kind.as_str() {
                                    "offer" => {
                                        sdp.map(|sdp| voice::RemoteSignal::Offer { from, sdp })
                                    }
                                    "answer" => sdp.map(|sdp| voice::RemoteSignal::Answer { sdp }),
                                    "candidate" => {
                                        candidate.map(|candidate| voice::RemoteSignal::Candidate {
                                            candidate,
                                            sdp_mid,
                                            sdp_mline_index,
                                        })
                                    }
                                    "end" => Some(voice::RemoteSignal::End),
                                    _ => None,
                                };
                                if let Some(remote) = remote {
                                    let _ = voice.send(voice::Command::Remote(remote));
                                }
                            }
                            RoomMessage::Error { message } => set_status(&weak, &message),
                        }
                    }
                }
                Ok(Message::Ping(_)) => {
                    let _ = socket.flush();
                }
                Ok(Message::Close(_)) => disconnected = true,
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    set_status(&weak, &format!("Watch room reconnecting… ({error})"));
                    disconnected = true;
                }
            }
        }

        set_status(&weak, "Watch room disconnected · reconnecting…");
        if wait_for_room_retry(&receiver, &mut pending, retry_delay) {
            return;
        }
    }
}

struct RoomSnapshot {
    revision: u64,
    source_kind: Option<String>,
    source: Option<String>,
    title: Option<String>,
    playing: bool,
    position_ms: u64,
    updated_by: Option<String>,
}

fn drain_outbound(
    receiver: &Receiver<RoomOutbound>,
    pending: &mut Vec<serde_json::Value>,
) -> Result<bool, ()> {
    loop {
        match receiver.try_recv() {
            Ok(RoomOutbound::Message(message)) => pending.push(message),
            Ok(RoomOutbound::Stop) => return Ok(true),
            Err(TryRecvError::Empty) => return Ok(false),
            Err(TryRecvError::Disconnected) => return Err(()),
        }
    }
}

fn wait_for_room_retry(
    receiver: &Receiver<RoomOutbound>,
    pending: &mut Vec<serde_json::Value>,
    delay: Duration,
) -> bool {
    let until = Instant::now() + delay;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        match receiver.recv_timeout(left.min(Duration::from_millis(250))) {
            Ok(RoomOutbound::Message(message)) => pending.push(message),
            Ok(RoomOutbound::Stop) | Err(RecvTimeoutError::Disconnected) => return true,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn connect_room_socket(
    base: &str,
    token: &str,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let endpoint = room_socket_url(base)?;
    let mut request = endpoint
        .into_client_request()
        .map_err(|error| error.to_string())?;
    request.headers_mut().insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|error| error.to_string())?,
    );
    tungstenite::connect(request)
        .map(|(socket, _)| socket)
        .map_err(|error| error.to_string())
}

fn room_socket_url(base: &str) -> Result<String, String> {
    let mut url = Url::parse(base).map_err(|error| error.to_string())?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("The watch room requires a secure HTTPS server address.".into());
    }
    url.set_scheme("wss")
        .map_err(|_| "Could not build the secure watch room URL.".to_owned())?;
    let prefix = url.path().trim_end_matches('/');
    url.set_path(&format!("{prefix}/api/v1/room/ws"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn set_socket_read_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, timeout: Duration) {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => {
            let _ = stream.set_read_timeout(Some(timeout));
        }
        MaybeTlsStream::Rustls(stream) => {
            let _ = stream.sock.set_read_timeout(Some(timeout));
        }
        _ => {}
    }
}

fn apply_room_state(
    session: &Session,
    state: &RoomSnapshot,
    playback: &SharedPlayback,
    weak: &slint::Weak<MainWindow>,
) {
    let (Some(source_kind), Some(source)) = (&state.source_kind, &state.source) else {
        return;
    };
    let is_live = source_kind == "live";
    let source_url = if source_kind == "library" {
        match stream_url(session, source) {
            Ok(url) => url,
            Err(error) => {
                set_status(weak, &error);
                return;
            }
        }
    } else if source_kind == "https" {
        if let Err(error) = validate_https(source) {
            set_status(weak, &error);
            return;
        }
        source.clone()
    } else if is_live {
        match live_hls_url(session, source) {
            Ok(url) => url,
            Err(error) => {
                set_status(weak, &error);
                return;
            }
        }
    } else {
        return;
    };

    let received_at = Instant::now();
    let target_position_ms = if is_live { 0 } else { state.position_ms };
    let key = format!("{source_kind}:{source}");
    let mut current = playback.lock().expect("playback lock poisoned");
    let needs_start = current.as_mut().map_or(true, |active| {
        active.key != key || active.child.try_wait().ok().flatten().is_some()
    });
    if needs_start {
        if let Some(mut previous) = current.take() {
            let _ = previous.child.kill();
            let _ = previous.child.wait();
        }
        let subtitle_urls = if source_kind == "library" {
            list_subtitle_urls(session, source)
        } else {
            Vec::new()
        };
        match launch_player(
            &source_url,
            (source_kind == "library" || is_live).then_some(session.token.as_str()),
            &subtitle_urls,
            target_position_ms,
            !state.playing,
            is_live,
        ) {
            Ok((child, endpoint)) => {
                *current = Some(Playback {
                    key: key.clone(),
                    revision: state.revision,
                    endpoint: endpoint.clone(),
                    child,
                    playing: state.playing,
                    position_ms: target_position_ms,
                    room_position_ms: target_position_ms,
                    room_anchor: received_at,
                    room_playing: state.playing,
                    sync_speed: 1.0,
                    suppress_local_until: Instant::now() + Duration::from_millis(1500),
                    is_live,
                });
                start_player_monitor(endpoint, key, session.room.clone(), playback.clone());
            }
            Err(error) => {
                set_status(weak, &error);
                return;
            }
        }
    } else if let Some(active) = current.as_mut() {
        if active.revision == state.revision {
            // The server sends clock snapshots without changing the command revision.
            // Refresh the monotonic anchor without forcing a seek or overriding local UI.
            active.room_position_ms = target_position_ms;
            active.room_anchor = received_at;
            active.room_playing = state.playing;
            return;
        }
        if active.is_live {
            let _ = mpv_command(
                &active.endpoint,
                serde_json::json!(["set_property", "pause", !state.playing]),
            );
            active.playing = state.playing;
            active.room_playing = state.playing;
            active.revision = state.revision;
            active.suppress_local_until = Instant::now() + Duration::from_millis(1500);
            let label = if state.playing { "Live" } else { "Paused" };
            set_status(
                weak,
                &format!(
                    "{label} desktop stream · {}",
                    state.title.as_deref().unwrap_or("OBS")
                ),
            );
            return;
        }
        let _ = mpv_command(
            &active.endpoint,
            serde_json::json!(["seek", target_position_ms as f64 / 1000.0, "absolute+exact"]),
        );
        let _ = mpv_command(
            &active.endpoint,
            serde_json::json!(["set_property", "pause", !state.playing]),
        );
        let _ = mpv_command(
            &active.endpoint,
            serde_json::json!(["set_property", "speed", 1.0]),
        );
        active.playing = state.playing;
        active.position_ms = target_position_ms;
        active.room_position_ms = target_position_ms;
        active.room_anchor = received_at;
        active.room_playing = state.playing;
        active.sync_speed = 1.0;
        active.suppress_local_until = Instant::now() + Duration::from_millis(1500);
        active.revision = state.revision;
    }
    let title = state.title.as_deref().unwrap_or("Shared video");
    let label = if state.playing { "Playing" } else { "Paused" };
    let by = state
        .updated_by
        .as_deref()
        .map(|username| format!(" · {username}"))
        .unwrap_or_default();
    let position = target_position_ms;
    if let Some(active) = current.as_mut() {
        active.position_ms = position;
    }
    let message = format!(
        "Room {label} · {title}{by} · #{}, {}",
        state.revision,
        format_time(position)
    );
    set_status(weak, &message);
}

fn format_time(position_ms: u64) -> String {
    let seconds = position_ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn expected_room_position_ms(
    position_ms: u64,
    playing: bool,
    anchor: Instant,
    now: Instant,
) -> u64 {
    if playing {
        position_ms.saturating_add(now.saturating_duration_since(anchor).as_millis() as u64)
    } else {
        position_ms
    }
}

fn room_sync_speed(local_position_ms: u64, target_position_ms: u64, playing: bool) -> f64 {
    if !playing {
        return 1.0;
    }
    if target_position_ms.saturating_sub(local_position_ms) > 90 {
        1.04
    } else if local_position_ms.saturating_sub(target_position_ms) > 90 {
        0.96
    } else {
        1.0
    }
}

fn list_items(base: &str, token: &str, path: &str) -> Result<Vec<ApiItem>, String> {
    let mut url = Url::parse(&format!("{base}/api/v1/media")).map_err(|error| error.to_string())?;
    url.query_pairs_mut().append_pair("path", path);
    let mut response = ureq::get(url.as_str())
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|error| error.to_string())?;
    let library: LibraryReply = response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())?;
    Ok(library.items)
}

fn list_subtitle_urls(session: &Session, media_path: &str) -> Vec<String> {
    let Ok(mut address) = Url::parse(&format!("{}/api/v1/subtitles", session.base)) else {
        return Vec::new();
    };
    address.query_pairs_mut().append_pair("media", media_path);
    let Ok(mut response) = ureq::get(address.as_str())
        .header("Authorization", &format!("Bearer {}", session.token))
        .call()
    else {
        return Vec::new();
    };
    let Ok(reply) = response.body_mut().read_json::<SubtitleReply>() else {
        return Vec::new();
    };
    reply
        .subtitles
        .into_iter()
        .filter_map(|subtitle| stream_url(session, &subtitle.path).ok())
        .collect()
}

fn refresh(
    session: &Arc<Mutex<Option<Session>>>,
    folder: &Arc<Mutex<String>>,
    weak: &slint::Weak<MainWindow>,
) {
    let Some(active) = session.lock().expect("session lock poisoned").clone() else {
        return;
    };
    let path = folder.lock().expect("folder lock poisoned").clone();
    set_status(weak, "Refreshing library…");
    let weak = weak.clone();
    thread::spawn(move || {
        let result = list_items(&active.base, &active.token, &path);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(items) => {
                        ui.set_media_items(to_model(items));
                        ui.set_status_text(SharedString::default());
                    }
                    Err(message) => ui.set_status_text(message.into()),
                }
            }
        });
    });
}

fn to_model(items: Vec<ApiItem>) -> ModelRc<MediaItem> {
    let rows = items
        .into_iter()
        .map(|item| MediaItem {
            name: item.name.into(),
            path: item.path.into(),
            kind: item.kind.into(),
            size: format_size(item.size).into(),
        })
        .collect::<Vec<_>>();
    ModelRc::new(VecModel::from(rows))
}

fn stream_url(session: &Session, path: &str) -> Result<String, String> {
    let mut url = Url::parse(&format!("{}/api/v1/stream", session.base))
        .map_err(|error| error.to_string())?;
    url.query_pairs_mut().append_pair("path", path);
    Ok(url.to_string())
}

fn live_hls_url(session: &Session, path: &str) -> Result<String, String> {
    let mut segments = path.split('/');
    let app = segments.next().unwrap_or_default();
    let key = segments.next().unwrap_or_default();
    if app != "mabaeiream"
        || segments.next().is_some()
        || !(32..=128).contains(&key.len())
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("The live desktop stream path is invalid.".into());
    }
    let mut url = Url::parse(&format!(
        "{}/api/v1/live/hls",
        session.base.trim_end_matches('/')
    ))
    .map_err(|error| error.to_string())?;
    url.path_segments_mut()
        .map_err(|_| "Could not build the live video address.".to_owned())?
        .push(app)
        .push(key)
        .push("index.m3u8");
    Ok(url.to_string())
}

fn launch_player(
    url: &str,
    token: Option<&str>,
    subtitle_urls: &[String],
    position_ms: u64,
    paused: bool,
    is_live: bool,
) -> Result<(std::process::Child, String), String> {
    let executable = env::current_exe()
        .map_err(|error| format!("Could not locate the MaBaeiream app folder: {error}"))?;
    let (runtime_dir, player, yt_dlp) = player_paths(&executable, cfg!(target_os = "windows"))?;
    if !player.is_file() {
        return Err(
            "MaBaeiream's bundled playback files are missing. Re-extract the full app package."
                .into(),
        );
    }

    #[cfg(unix)]
    for path in [&player, &yt_dlp] {
        if path.is_file() {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(path)
                .map_err(|error| format!("Could not read bundled playback file: {error}"))?;
            let mut permissions = metadata.permissions();
            if permissions.mode() & 0o111 == 0 {
                permissions.set_mode(permissions.mode() | 0o111);
                std::fs::set_permissions(path, permissions).map_err(|error| {
                    format!("Could not prepare MaBaeiream's bundled playback files: {error}")
                })?;
            }
        }
    }

    let endpoint = create_ipc_endpoint();
    let mut command = Command::new(&player);
    let mut search_paths = vec![runtime_dir];
    if let Some(existing) = env::var_os("PATH") {
        search_paths.extend(env::split_paths(&existing));
    }
    if let Ok(path) = env::join_paths(search_paths) {
        command.env("PATH", path);
    }
    command.args(["--no-config", "--force-window"]);
    command.arg(format!("--input-ipc-server={endpoint}"));
    if !is_live {
        command.arg(format!("--start={:.3}", position_ms as f64 / 1000.0));
    }
    if paused {
        command.arg("--pause=yes");
    }
    // The portable Linux AppImage ships an optional self-updater that can ask
    // users to download and approve updates. MaBaeiream packages its runtime,
    // so keep playback fully silent and let app releases update it instead.
    #[cfg(target_os = "linux")]
    command.env("DISABLE_AUTO_UPDATES", "1");
    if yt_dlp.is_file() {
        command.arg(format!(
            "--script-opts=ytdl_hook-ytdl_path={}",
            yt_dlp.display()
        ));
    }
    if let Some(token) = token {
        command.arg(format!(
            "--http-header-fields=Authorization: Bearer {token}"
        ));
    }
    for subtitle_url in subtitle_urls {
        command.arg(format!("--sub-file={subtitle_url}"));
    }
    let child = command.arg("--").arg(url).spawn().map_err(|_| {
        "MaBaeiream could not start its bundled playback engine. Re-extract the full app package."
            .to_owned()
    })?;
    Ok((child, endpoint))
}

fn create_ipc_endpoint() -> String {
    let id = NEXT_IPC_ID.fetch_add(1, Ordering::Relaxed);
    #[cfg(target_os = "windows")]
    {
        format!(r"\\.\pipe\mabaeiream-{}-{id}", std::process::id())
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("/tmp/mabaeiream-{}-{id}.sock", std::process::id())
    }
}

trait IpcReadWrite: Read + Write {}
impl<T: Read + Write> IpcReadWrite for T {}

#[cfg(unix)]
fn open_ipc(endpoint: &str) -> std::io::Result<Box<dyn IpcReadWrite>> {
    use std::os::unix::net::UnixStream;
    let stream = UnixStream::connect(endpoint)?;
    stream.set_read_timeout(Some(Duration::from_millis(700)))?;
    Ok(Box::new(stream))
}

#[cfg(windows)]
fn open_ipc(endpoint: &str) -> std::io::Result<Box<dyn IpcReadWrite>> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(endpoint)
        .map(|stream| Box::new(stream) as Box<dyn IpcReadWrite>)
}

fn mpv_command(endpoint: &str, command: serde_json::Value) -> std::io::Result<()> {
    let request_id = NEXT_IPC_ID.fetch_add(1, Ordering::Relaxed);
    let mut stream = open_ipc(endpoint)?;
    let request = serde_json::json!({"command": command, "request_id": request_id});
    writeln!(stream, "{request}")?;
    stream.flush()?;
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response)?;
    Ok(())
}

fn mpv_command_retry(endpoint: &str, command: serde_json::Value, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if mpv_command(endpoint, command.clone()).is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn mpv_property(endpoint: &str, property: &str) -> Option<serde_json::Value> {
    let request_id = NEXT_IPC_ID.fetch_add(1, Ordering::Relaxed);
    let mut stream = open_ipc(endpoint).ok()?;
    let request = serde_json::json!({
        "command": ["get_property", property],
        "request_id": request_id,
    });
    writeln!(stream, "{request}").ok()?;
    stream.flush().ok()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    let response: serde_json::Value = serde_json::from_str(&line).ok()?;
    (response.get("request_id")?.as_u64()? == request_id).then(|| response.get("data").cloned())?
}

fn handle_missing_player_property(
    endpoint: &str,
    key: &str,
    room: &Sender<RoomOutbound>,
    playback: &SharedPlayback,
) -> bool {
    let mut current = playback.lock().expect("playback lock poisoned");
    let Some(active) = current
        .as_mut()
        .filter(|active| active.key == key && active.endpoint == endpoint)
    else {
        return false;
    };
    match active.child.try_wait() {
        Ok(None) => true,
        Ok(Some(_)) => {
            let position_ms = active.position_ms;
            let _ = room.send(RoomOutbound::Message(serde_json::json!({
                "type": "pause",
                "position_ms": position_ms,
            })));
            current.take();
            false
        }
        Err(_) => false,
    }
}

fn start_player_monitor(
    endpoint: String,
    key: String,
    room: Sender<RoomOutbound>,
    playback: SharedPlayback,
) {
    thread::spawn(move || {
        let mut previous: Option<(u64, bool, Instant)> = None;
        loop {
            thread::sleep(Duration::from_millis(350));
            let (still_current, is_live, finished) = {
                let mut current = playback.lock().expect("playback lock poisoned");
                let Some(active) = current
                    .as_mut()
                    .filter(|active| active.key == key && active.endpoint == endpoint)
                else {
                    return;
                };
                (
                    true,
                    active.is_live,
                    active.child.try_wait().ok().flatten().is_some(),
                )
            };
            if !still_current || finished {
                return;
            }
            if is_live {
                continue;
            }

            let Some(position) = mpv_property(&endpoint, "time-pos").and_then(|v| v.as_f64())
            else {
                if handle_missing_player_property(&endpoint, &key, &room, &playback) {
                    continue;
                }
                return;
            };
            let Some(paused) = mpv_property(&endpoint, "pause").and_then(|v| v.as_bool()) else {
                if handle_missing_player_property(&endpoint, &key, &room, &playback) {
                    continue;
                }
                return;
            };
            let position_ms = (position.max(0.0) * 1000.0).round() as u64;
            let now = Instant::now();
            let (suppressed, room_playing, target_position_ms, current_speed) = {
                let mut current = playback.lock().expect("playback lock poisoned");
                let Some(active) = current
                    .as_mut()
                    .filter(|active| active.key == key && active.endpoint == endpoint)
                else {
                    return;
                };
                let target = expected_room_position_ms(
                    active.room_position_ms,
                    active.room_playing,
                    active.room_anchor,
                    now,
                );
                let suppressed = active.suppress_local_until > now;
                active.position_ms = position_ms;
                active.playing = !paused;
                (suppressed, active.room_playing, target, active.sync_speed)
            };

            if suppressed {
                previous = Some((position_ms, paused, now));
                continue;
            }

            let Some((last_position_ms, last_paused, sampled_at)) = previous else {
                if paused != room_playing {
                    let aligned = mpv_command_retry(
                        &endpoint,
                        serde_json::json!(["set_property", "pause", !room_playing]),
                        Duration::from_secs(2),
                    );
                    if aligned {
                        if let Some(active) = playback
                            .lock()
                            .expect("playback lock poisoned")
                            .as_mut()
                            .filter(|active| active.key == key && active.endpoint == endpoint)
                        {
                            active.suppress_local_until =
                                Instant::now() + Duration::from_millis(1_200);
                        }
                    }
                    previous = None;
                } else {
                    previous = Some((position_ms, paused, now));
                }
                continue;
            };

            let sample_elapsed_ms = now.saturating_duration_since(sampled_at).as_millis() as u64;
            let expected_from_previous =
                last_position_ms.saturating_add(if last_paused { 0 } else { sample_elapsed_ms });
            let local_seek = position_ms.abs_diff(expected_from_previous) > 900;

            if paused != last_paused {
                let message = serde_json::json!({
                    "type": if paused { "pause" } else { "play" },
                    "position_ms": position_ms,
                });
                if room.send(RoomOutbound::Message(message)).is_err() {
                    return;
                }
                previous = Some((position_ms, paused, now));
                continue;
            }

            // The room is authoritative for remote play/pause changes. A command can
            // arrive before MPV creates its IPC socket; retry and keep local playback
            // from publishing a stale pause back into the room.
            if paused != room_playing {
                let aligned = mpv_command_retry(
                    &endpoint,
                    serde_json::json!(["set_property", "pause", !room_playing]),
                    Duration::from_secs(2),
                );
                if aligned {
                    if let Some(active) = playback
                        .lock()
                        .expect("playback lock poisoned")
                        .as_mut()
                        .filter(|active| active.key == key && active.endpoint == endpoint)
                    {
                        active.suppress_local_until = Instant::now() + Duration::from_millis(1_200);
                    }
                }
                previous = None;
                continue;
            }

            if local_seek {
                if room
                    .send(RoomOutbound::Message(serde_json::json!({
                        "type": "seek",
                        "position_ms": position_ms,
                    })))
                    .is_err()
                {
                    return;
                }
                previous = Some((position_ms, paused, now));
                continue;
            }

            let drift = position_ms.abs_diff(target_position_ms);
            if drift > 650 {
                let _ = mpv_command(
                    &endpoint,
                    serde_json::json!([
                        "seek",
                        target_position_ms as f64 / 1000.0,
                        "absolute+exact"
                    ]),
                );
                let _ = mpv_command(&endpoint, serde_json::json!(["set_property", "speed", 1.0]));
                if let Some(active) = playback
                    .lock()
                    .expect("playback lock poisoned")
                    .as_mut()
                    .filter(|active| active.key == key && active.endpoint == endpoint)
                {
                    active.position_ms = target_position_ms;
                    active.sync_speed = 1.0;
                    active.suppress_local_until = now + Duration::from_millis(1_200);
                }
                previous = None;
                continue;
            }

            let speed = room_sync_speed(position_ms, target_position_ms, room_playing);
            if (speed - current_speed).abs() > 0.005 {
                let _ = mpv_command(
                    &endpoint,
                    serde_json::json!(["set_property", "speed", speed]),
                );
                if let Some(active) = playback
                    .lock()
                    .expect("playback lock poisoned")
                    .as_mut()
                    .filter(|active| active.key == key && active.endpoint == endpoint)
                {
                    active.sync_speed = speed;
                }
            }
            previous = Some((position_ms, paused, now));
        }
    });
}

fn stop_playback(playback: &SharedPlayback) {
    if let Some(mut active) = playback.lock().expect("playback lock poisoned").take() {
        let _ = active.child.kill();
        let _ = active.child.wait();
    }
}

fn player_paths(executable: &Path, windows: bool) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let app_dir = executable
        .parent()
        .ok_or_else(|| "Could not locate the MaBaeiream app folder.".to_owned())?;
    let runtime_dir = app_dir.join("runtime").join("mpv");
    let (player_name, yt_dlp_name) = if windows {
        ("mpv.exe", "yt-dlp.exe")
    } else {
        ("mpv.AppImage", "yt-dlp")
    };
    Ok((
        runtime_dir.clone(),
        runtime_dir.join(player_name),
        runtime_dir.join(yt_dlp_name),
    ))
}

#[cfg(test)]
mod player_runtime_tests {
    use super::{
        Session, expected_room_position_ms, live_hls_url, player_paths, room_socket_url,
        room_sync_speed,
    };
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn packaged_player_and_ytdlp_are_resolved_next_to_the_app() {
        let (runtime, player, yt_dlp) =
            player_paths(Path::new("/opt/mabaeiream/MaBaeiream"), false).unwrap();
        assert_eq!(runtime, Path::new("/opt/mabaeiream/runtime/mpv"));
        assert_eq!(player, runtime.join("mpv.AppImage"));
        assert_eq!(yt_dlp, runtime.join("yt-dlp"));
    }

    #[test]
    fn windows_package_uses_exe_runtime_names() {
        let (runtime, player, yt_dlp) =
            player_paths(Path::new(r"C:\Apps\MaBaeiream.exe"), true).unwrap();
        assert_eq!(runtime, Path::new(r"C:\Apps\runtime\mpv"));
        assert_eq!(player, runtime.join("mpv.exe"));
        assert_eq!(yt_dlp, runtime.join("yt-dlp.exe"));
    }

    #[test]
    fn room_socket_uses_secure_scheme_and_preserves_server_route_prefix() {
        assert_eq!(
            room_socket_url("https://ahura.site/mabaeiream/").unwrap(),
            "wss://ahura.site/mabaeiream/api/v1/room/ws"
        );
        assert!(room_socket_url("http://ahura.site/mabaeiream").is_err());
    }

    #[test]
    fn room_clock_advances_only_while_playing() {
        let anchor = Instant::now();
        assert_eq!(
            expected_room_position_ms(10_000, true, anchor, anchor + Duration::from_secs(3)),
            13_000
        );
        assert_eq!(
            expected_room_position_ms(10_000, false, anchor, anchor + Duration::from_secs(3)),
            10_000
        );
    }

    #[test]
    fn live_hls_url_contains_only_the_configured_path_shape() {
        let session = Session {
            base: "https://ahura.site/mabaeiream".into(),
            token: "test-token".into(),
            room: mpsc::channel().0,
            voice: mpsc::channel().0,
        };
        assert_eq!(
            live_hls_url(&session, "mabaeiream/abcdefghijklmnopqrstuvwxyz123456").unwrap(),
            "https://ahura.site/mabaeiream/api/v1/live/hls/mabaeiream/abcdefghijklmnopqrstuvwxyz123456/index.m3u8",
        );
        assert!(live_hls_url(&session, "mabaeiream/../../secret").is_err());
    }

    #[test]
    fn room_sync_rate_corrects_both_sides_and_returns_to_normal() {
        assert_eq!(room_sync_speed(9_000, 10_000, true), 1.04);
        assert_eq!(room_sync_speed(11_000, 10_000, true), 0.96);
        assert_eq!(room_sync_speed(10_000, 10_000, true), 1.0);
        assert_eq!(room_sync_speed(9_000, 10_000, false), 1.0);
    }
}

fn validate_https(value: &str) -> Result<(), String> {
    let url = Url::parse(value).map_err(|_| "Enter a valid HTTPS link.".to_owned())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("MaBaeiream requires an HTTPS link without embedded credentials.".into());
    }
    Ok(())
}

fn validate_download_url(value: &str) -> Result<(), String> {
    let url = Url::parse(value).map_err(|_| "Enter a valid HTTP or HTTPS file link.".to_owned())?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Use a public HTTP or HTTPS link without embedded credentials.".into());
    }
    Ok(())
}

fn post_logout(session: &Session) -> Result<(), String> {
    ureq::post(&format!("{}/api/v1/auth/logout", session.base))
        .header("Authorization", &format!("Bearer {}", session.token))
        .send_empty()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn queue_download(session: &Session, address: &str) -> Result<String, String> {
    let mut response = ureq::post(&format!("{}/api/v1/downloads", session.base))
        .header("Authorization", &format!("Bearer {}", session.token))
        .send_json(serde_json::json!({"url": address}))
        .map_err(|error| error.to_string())?;
    let job: DownloadReply = response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())?;
    Ok(job.id)
}

fn poll_download(session: &Session, id: &str, weak: &slint::Weak<MainWindow>) -> Option<String> {
    for _ in 0..1800 {
        thread::sleep(Duration::from_millis(600));
        let mut response = ureq::get(&format!("{}/api/v1/downloads/{id}", session.base))
            .header("Authorization", &format!("Bearer {}", session.token))
            .call()
            .ok()?;
        let job: DownloadReply = response.body_mut().read_json().ok()?;
        let status = if let Some(total) = job.total.filter(|value| *value > 0) {
            format!("Downloading · {}%", job.bytes.saturating_mul(100) / total)
        } else {
            format!("Downloading · {}", format_size(job.bytes))
        };
        let state = job.state.clone();
        let message = if state == "complete" {
            format!(
                "Saved to {}",
                job.saved_as.unwrap_or_else(|| "the library".into())
            )
        } else if state == "failed" {
            job.error.unwrap_or_else(|| "Download failed.".into())
        } else if state == "cancelled" {
            "Download cancelled.".into()
        } else {
            status
        };
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || set_status(&weak, &message));
        if state == "complete" || state == "failed" || state == "cancelled" {
            return Some(state);
        }
    }
    set_status(
        weak,
        "Download is still running; refresh the library to check it.",
    );
    None
}

fn cancel_download_job(session: &Session, id: &str) -> Result<(), String> {
    ureq::post(&format!("{}/api/v1/downloads/{id}/cancel", session.base))
        .header("Authorization", &format!("Bearer {}", session.token))
        .send_empty()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn set_status(weak: &slint::Weak<MainWindow>, value: &str) {
    let value = value.to_owned();
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_status_text(value.into());
        }
    });
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.0} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}
