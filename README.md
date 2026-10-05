# MaBaeiream

MaBaeiream is a private two-person media library and watch room with native Windows, Linux, and Android clients. The current source includes authenticated shared playback and audio-only WebRTC voice calling. Slint powers the desktop UI; Jetpack Compose and Media3 power Android; a compact Rust/Axum service with SQLite runs on the server.

## What is in this source tree now

- `server/`: small Axum service backed by SQLite, with Argon2 password hashes, opaque expiring bearer sessions, two-account provisioning, directory browsing, byte-range media streaming, a queued direct-file downloader, and provider-link resolution.
- `android/`: native Kotlin/Jetpack Compose app with Media3 playback, HTTPS sign-in, server library browsing, system-picker uploads, shared playback, and foreground-only WebRTC voice calls.
- `desktop/`: native Slint Windows/Linux client with local file upload, shared playback, Opus voice chat, and bundled mpv/yt-dlp media runtimes.
- `design-system/MASTER.md`: the warm visual and interaction contract shared by the native clients.

### Playback across clients

Library files and direct MP4/HLS/DASH links synchronize across the room. The server sends anchored position snapshots twice per second; each native player uses its monotonic clock, gently corrects small drift, and seeks back into sync after a larger delay. Embedded subtitles and matching sidecar `.srt`, `.vtt`, `.ttml`, `.ssa`, or `.ass` files are available in the players' subtitle controls. Android resolves YouTube/Vimeo URLs with the server's `yt-dlp` binary and uses Media3 to combine separate provider video and audio tracks when needed; Windows and Linux use their bundled player and `yt-dlp` runtime. Network and decoder startup differences can still cause brief drift while a stream buffers.

### Live desktop streaming

OBS can publish to the room with WHIP over HTTPS. Streamlabs uses its Custom Streaming Server setting with the provided RTMPS server URL and stream key. MediaMTX accepts only the configured private publish path and authenticated app sessions can read the low-latency HLS output. The clients use their native Media3/mpv players, so playback is compatible across Android, Windows, and Linux; HLS adds some delay compared with a WebRTC receiver, and actual latency depends on the encoder, network, and device.

The MediaMTX service is configured in `ops/mediamtx/mediamtx.yml` and runs as a separate, restricted systemd service. RTMPS uses TCP 1936; WHIP signaling uses the HTTPS proxy, with WebRTC media on UDP 8189. Keep the plain RTMP port closed. The deploy setup copies the existing `ahura.site` TLS files into a root-owned, MaBaeiream-group-readable directory; install `ops/letsencrypt/mabaeiream-mediamtx` as a Certbot deploy hook so renewed copies and the relay restart together. Configure `MABAEIREAM_LIVE_STREAM_KEY` with a random URL-safe secret before enabling it.

### Voice chat

Voice uses WebRTC with Opus and authenticated signaling through the room socket. Clients retrieve authenticated ICE-server settings when joining, including 12-hour, user-bound TURN credentials so calls can relay media when direct peer-to-peer ICE is blocked. The server requires coturn configured with `MABAEIREAM_TURN_URLS` and `MABAEIREAM_TURN_SECRET`; see `ops/turn/install-turn.sh`. Native audio setup is delayed until a call starts, and Android uses libwebrtc's built-in processing to avoid device-specific hardware-effect failures. Desktop capture uses lock-free audio queues, skips missed frame deadlines, and drops stale capture backlog to limit latency. Desktop audio now lets the host choose its shared-mode buffer period instead of forcing a period that can trigger WASAPI/ALSA XRUNs; callback errors are reported away from the realtime audio thread, though severe CPU or device stalls can still cause glitches. Desktop software echo cancellation is not included, so a headset is recommended.

Call progress includes local ringback for the caller and a ringtone for the recipient. Android starts on loudspeaker by default, reapplies that route after WebRTC initializes and connects, and lets the user change the active call's audio route.

This is not a web app. It does not embed a website for its navigation or main interface.

## Server

The service listens on `127.0.0.1:9781` by default so it cannot be reached from the public network before a TLS reverse proxy is configured. Set:

The Linux server build at `artifacts/linux-x64/mabaeiream-server-linux-x64` is compiled on Ubuntu 24.04 and expects glibc 2.39 or newer.

- `MABAEIREAM_LISTEN` to the local proxy-facing bind address.
- `MABAEIREAM_MEDIA_DIR` to the library root.
- `MABAEIREAM_DB` to a persistent SQLite file.
- `MABAEIREAM_MAX_DOWNLOAD_BYTES` to cap a single server-side import (defaults to 20 GiB).
- `MABAEIREAM_MAX_UPLOAD_BYTES` to cap one app upload (defaults to 20 GiB); the Nginx upload location also allows up to 20 GiB and streams requests without buffering them to disk.
- `MABAEIREAM_YTDLP` to the `yt-dlp` executable used for Android provider-link playback (defaults to `yt-dlp` on `PATH`).

From this directory, provision each of the two accounts with a hidden password prompt, then start the API:

~~~sh
cargo run --release -p mabaeiream-server -- add-user first-account
cargo run --release -p mabaeiream-server -- add-user companion
cargo run --release -p mabaeiream-server -- serve
~~~

Passwords are not accepted as command-line arguments or stored in this repository. Put a TLS reverse proxy in front of the service before connecting a mobile or desktop client. The Android app rejects plain HTTP and does not disable certificate checks.

The file API rejects traversal and symlinks, checks the resolved file remains inside the configured library, and streams in bounded chunks with HTTP Range support. The link importer allows only HTTP(S) on standard web ports, resolves and pins public IP addresses, rejects private/special-use destinations on each redirect, limits redirects, and writes partial files under a temporary suffix before renaming them into the library. Downloads run one at a time. Completed downloads appear under `downloads/`.

## Android

The Android app uses Jetpack Compose and Media3 rather than a WebView. It targets API 36 and compiles against SDK 37. Its manifest requests `INTERNET`, `MODIFY_AUDIO_SETTINGS`, and `RECORD_AUDIO`; Android shows the microphone prompt only when the user starts or accepts a call. Calls end when the app leaves the foreground, so no microphone foreground service is needed. Google Play's target API policy is [here](https://support.google.com/googleplay/android-developer/answer/11926878?hl=en).

The Android source is version 0.3.9 (version code 12); the latest installable sideload APK is `artifacts/android/mabaeiream-android-0.3.9-sideload-signed-r17.apk`. To build it, open `android/` in Android Studio or install Gradle 9.6, Android SDK 37, and JDK 17 or newer. In this workspace, use `gradle :app:assembleRelease -x lintVitalAnalyzeRelease -x lintVitalReportRelease -x lintVitalRelease` because Android Gradle's release-lint classpath currently requests an unavailable Compose desktop artifact. The APK uses this workspace's debug certificate; it can update only a matching signature and is not a Play upload. A Play release needs the app owner's upload key and Play Console setup.

## Desktop

The desktop UI uses Slint and Rust. The Windows and Linux release packages include portable `mpv` and `yt-dlp` runtimes beside the app. Playback resolves these bundled files directly; recipients do not need to install a player, add anything to `PATH`, or approve a runtime download/update prompt. Updates to bundled runtimes ship with MaBaeiream releases. YouTube website links use the bundled `yt-dlp`. The library bearer token is sent only to MaBaeiream's own media stream endpoint and never to third-party links. The native file manager supports search, folder navigation and creation, local uploads, and downloads from direct links; Android's upload action uses the system document picker and requests no broad storage permission. The Watch screen in each client shows the OBS/Streamlabs connection details and shared live status. Distribute the complete platform ZIP from `artifacts/`, not the standalone executable.

The latest packaged desktop builds are r16, distributed as `artifacts/mabaeiream-windows-x64-r16.zip` and `artifacts/mabaeiream-linux-x64-r16.zip`. The Linux client is a native Slint application built on Ubuntu 24.04 and needs glibc 2.39+, ALSA, and an X11 or Wayland desktop session. Extract the full Linux ZIP and start it with `sh ./run-mabaeiream.sh`; the launcher prepares executable permissions for the bundled files. This is a portable app bundle, not a system installer.

Build on each target OS with `cargo build --release --manifest-path desktop/Cargo.toml`, then use `ops/package-desktop.ps1` to create distributable ZIPs from the bundled runtime files and compiled apps. Linux builds need the platform development libraries required by Slint's window backend. Windows and Linux binaries target their respective operating systems and are not interchangeable.

## Known limitations

Cross-network calling still needs physical-device validation, and the VPS TURN service must be reachable on TCP/UDP 3478, TCP 5349, and UDP relay ports 49160–49200. Native players continuously correct playback drift against the room's server clock. A Play upload still needs an owner-controlled signing key and Play Console setup. See `PLAY-RELEASE.md` for the Android configuration and remaining release checks.


