use crate::{MainWindow, RoomOutbound};
use cpal::{
    FromSample, Sample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crossbeam_queue::ArrayQueue;
use opus::{Application, Bitrate, Channels, Decoder, Encoder};
use rtc::{
    interceptor::Registry,
    media::Sample as MediaSample,
    media_stream::MediaStreamTrack,
    peer_connection::configuration::media_engine::MIME_TYPE_OPUS,
    peer_connection::configuration::{
        interceptor_registry::register_default_interceptors, media_engine::MediaEngine,
    },
    rtp_transceiver::rtp_sender::{
        RTCRtpCodec, RTCRtpCodingParameters, RTCRtpEncodingParameters, RtpCodecKind,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc::Sender,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::runtime::Builder;
use webrtc::{
    media_stream::{
        track_local::{TrackLocal, static_sample::TrackLocalStaticSample},
        track_remote::{TrackRemote, TrackRemoteEvent},
    },
    peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder,
        RTCIceCandidateInit, RTCIceServer, RTCPeerConnectionIceEvent, RTCPeerConnectionState,
        RTCSessionDescription,
    },
};

const AUDIO_RATE: u32 = 48_000;
const FRAME_SAMPLES: usize = 960;
const FRAME_TIME: Duration = Duration::from_millis(20);
const AUDIO_QUEUE_SAMPLES: usize = 48_000;
const AUDIO_SSRC: u32 = 2_417_039;
const OPUS_PAYLOAD_TYPE: u8 = 111;
const TONE_NONE: u8 = 0;
const TONE_RINGBACK: u8 = 1;
const TONE_INCOMING: u8 = 2;

#[derive(Debug)]
pub(super) enum RemoteSignal {
    Offer {
        from: String,
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

pub(super) enum Command {
    Call,
    Accept,
    Decline,
    End,
    Remote(RemoteSignal),
    Stop,
}

pub(super) fn spawn(
    room_sender: Sender<RoomOutbound>,
    window: slint::Weak<MainWindow>,
    api_base: String,
    token: String,
) -> Sender<Command> {
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::Builder::new()
        .name("mabaeiream-webrtc-voice".into())
        .spawn(move || {
            let runtime = match Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    update_ui(&window, "failed", &format!("Voice runtime failed: {error}"));
                    return;
                }
            };
            let mut call = VoiceCall::new(room_sender, window, api_base, token);
            while let Ok(command) = receiver.recv() {
                if matches!(command, Command::Stop) {
                    runtime.block_on(call.end(true));
                    break;
                }
                runtime.block_on(call.handle(command));
            }
        })
        .expect("could not start voice manager");
    sender
}

struct VoiceCall {
    room_sender: Sender<RoomOutbound>,
    window: slint::Weak<MainWindow>,
    peer: Option<Arc<dyn PeerConnection>>,
    peer_failed: Arc<AtomicBool>,
    audio: Option<AudioIo>,
    capture: Arc<ArrayQueue<f32>>,
    playback: Arc<ArrayQueue<f32>>,
    sample_rate: u32,
    pending_offer: Option<(String, String)>,
    pending_candidates: Vec<RTCIceCandidateInit>,
    tone_mode: Arc<AtomicU8>,
    incoming_ringtone: Option<cpal::Stream>,
    api_base: String,
    token: String,
    ice_servers: Option<Vec<RTCIceServer>>,
}

impl VoiceCall {
    fn new(
        room_sender: Sender<RoomOutbound>,
        window: slint::Weak<MainWindow>,
        api_base: String,
        token: String,
    ) -> Self {
        Self {
            room_sender,
            window,
            peer: None,
            peer_failed: Arc::new(AtomicBool::new(false)),
            audio: None,
            capture: Arc::new(ArrayQueue::new(AUDIO_QUEUE_SAMPLES)),
            playback: Arc::new(ArrayQueue::new(AUDIO_QUEUE_SAMPLES)),
            sample_rate: AUDIO_RATE,
            pending_offer: None,
            pending_candidates: Vec::new(),
            tone_mode: Arc::new(AtomicU8::new(TONE_NONE)),
            incoming_ringtone: None,
            api_base,
            token,
            ice_servers: None,
        }
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::Call => self.start_outgoing().await,
            Command::Accept => self.accept_incoming().await,
            Command::Decline => {
                let _ = self.send_signal(json!({"kind":"end"}));
                self.end(false).await;
            }
            Command::End => {
                if self.peer.is_some() || self.pending_offer.is_some() {
                    let _ = self.send_signal(json!({"kind":"end"}));
                }
                self.end(false).await;
            }
            Command::Remote(signal) => self.remote_signal(signal).await,
            Command::Stop => self.end(true).await,
        }
    }

    async fn start_outgoing(&mut self) {
        if self.pending_offer.is_some() {
            return;
        }
        if self.peer.is_some() {
            self.end(false).await;
        }
        match self.create_peer().await {
            Ok(peer) => match peer.create_offer(None).await {
                Ok(offer) => match peer.set_local_description(offer).await {
                    Ok(()) => match peer.local_description().await {
                        Some(description) => {
                            self.set_state("calling", "Calling your watch partner…");
                            if let Err(error) =
                                self.send_signal(json!({"kind":"offer","sdp":description.sdp}))
                            {
                                self.fail(&error);
                            } else {
                                self.tone_mode.store(TONE_RINGBACK, Ordering::Release);
                            }
                        }
                        None => self.fail("Could not prepare the WebRTC offer."),
                    },
                    Err(error) => self.fail(&format!("Could not start voice: {error}")),
                },
                Err(error) => self.fail(&format!("Could not create a call: {error}")),
            },
            Err(error) => self.fail(&error),
        }
    }

    async fn accept_incoming(&mut self) {
        self.stop_call_tone();
        let Some((from, sdp)) = self.pending_offer.take() else {
            return;
        };
        match self.create_peer().await {
            Ok(peer) => {
                let offer = match RTCSessionDescription::offer(sdp) {
                    Ok(offer) => offer,
                    Err(error) => return self.fail(&format!("Invalid voice offer: {error}")),
                };
                if let Err(error) = peer.set_remote_description(offer).await {
                    return self.fail(&format!("Could not accept voice offer: {error}"));
                }
                self.apply_pending_candidates(&peer).await;
                match peer.create_answer(None).await {
                    Ok(answer) => match peer.set_local_description(answer).await {
                        Ok(()) => match peer.local_description().await {
                            Some(description) => {
                                self.set_state("connecting", "Connecting voice…");
                                if let Err(error) =
                                    self.send_signal(json!({"kind":"answer","sdp":description.sdp}))
                                {
                                    self.fail(&error);
                                }
                                let _ = from;
                            }
                            None => self.fail("Could not prepare the voice answer."),
                        },
                        Err(error) => self.fail(&format!("Could not answer the call: {error}")),
                    },
                    Err(error) => self.fail(&format!("Could not create a voice answer: {error}")),
                }
            }
            Err(error) => self.fail(&error),
        }
    }

    async fn remote_signal(&mut self, signal: RemoteSignal) {
        match signal {
            RemoteSignal::Offer { from, sdp } => {
                if should_reject_remote_offer(
                    self.peer.is_some(),
                    self.pending_offer.is_some(),
                    self.peer_failed.load(Ordering::Acquire),
                ) {
                    let _ = self.send_signal(json!({"kind":"end"}));
                    return;
                }
                // Failed peer connections remain allocated until hangup. Clear
                // one before receiving another offer so later calls are not
                // incorrectly rejected as busy.
                if self.peer.is_some() {
                    self.end(false).await;
                }
                self.start_incoming_ringtone();
                self.pending_offer = Some((from.clone(), sdp));
                self.set_state("incoming", &format!("{from} is calling"));
            }
            RemoteSignal::Answer { sdp } => {
                self.stop_call_tone();
                if let Some(peer) = self.peer.clone() {
                    match RTCSessionDescription::answer(sdp) {
                        Ok(answer) => {
                            if let Err(error) = peer.set_remote_description(answer).await {
                                self.fail(&format!("Could not connect the voice call: {error}"));
                            } else {
                                self.apply_pending_candidates(&peer).await;
                                self.set_state("connecting", "Connecting voice…");
                            }
                        }
                        Err(error) => self.fail(&format!("Invalid voice answer: {error}")),
                    }
                }
            }
            RemoteSignal::Candidate {
                candidate,
                sdp_mid,
                sdp_mline_index,
            } => {
                let init = RTCIceCandidateInit {
                    candidate,
                    sdp_mid,
                    sdp_mline_index,
                    username_fragment: None,
                    url: None,
                };
                if let Some(peer) = self.peer.clone() {
                    if peer.remote_description().await.is_some() {
                        if let Err(error) = peer.add_ice_candidate(init).await {
                            self.fail(&format!("Could not add a network candidate: {error}"));
                        }
                    } else {
                        self.pending_candidates.push(init);
                    }
                } else {
                    self.pending_candidates.push(init);
                }
            }
            RemoteSignal::End => self.end(false).await,
        }
    }

    async fn create_peer(&mut self) -> Result<Arc<dyn PeerConnection>, String> {
        if self.ice_servers.is_none() {
            let api_base = self.api_base.clone();
            let token = self.token.clone();
            let servers = tokio::task::spawn_blocking(move || fetch_ice_servers(&api_base, &token))
                .await
                .map_err(|error| format!("Could not load voice network settings: {error}"))??;
            self.ice_servers = Some(servers);
        }
        let audio = open_audio(
            self.capture.clone(),
            self.playback.clone(),
            self.tone_mode.clone(),
        )
        .map_err(|error| format!("Could not open microphone/speakers: {error}"))?;
        self.sample_rate = audio.input_rate;
        let diagnostics = audio.diagnostics.clone();
        self.audio = Some(audio);
        let track = self.make_audio_track()?;
        let peer_failed = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(VoiceHandler {
            room_sender: self.room_sender.clone(),
            playback: self.playback.clone(),
            window: self.window.clone(),
            peer_failed: peer_failed.clone(),
            tone_mode: self.tone_mode.clone(),
        });
        let configuration = RTCConfigurationBuilder::new()
            .with_ice_servers(self.ice_servers.clone().unwrap_or_default())
            .build();
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .map_err(|error| error.to_string())?;
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)
            .map_err(|error| error.to_string())?;
        let peer: Arc<dyn PeerConnection> = Arc::new(
            PeerConnectionBuilder::new()
                .with_configuration(configuration)
                .with_media_engine(media_engine)
                .with_interceptor_registry(registry)
                .with_handler(handler)
                .with_udp_addrs(vec!["0.0.0.0:0"])
                .build()
                .await
                .map_err(|error| error.to_string())?,
        );
        peer.add_track(track.clone() as Arc<dyn TrackLocal>)
            .await
            .map_err(|error| error.to_string())?;
        self.peer = Some(peer.clone());
        self.peer_failed = peer_failed;
        self.start_capture(track, self.sample_rate, diagnostics);
        self.set_state("connecting", "Connecting voice…");
        Ok(peer)
    }

    fn make_audio_track(&self) -> Result<Arc<TrackLocalStaticSample>, String> {
        let stream_id = format!("mabaeiream-{}", rand::random::<u32>());
        let track = MediaStreamTrack::new(
            stream_id.clone(),
            format!("{stream_id}-audio"),
            "MaBaeiream voice".to_owned(),
            RtpCodecKind::Audio,
            vec![RTCRtpEncodingParameters {
                rtp_coding_parameters: RTCRtpCodingParameters {
                    ssrc: Some(AUDIO_SSRC),
                    ..Default::default()
                },
                codec: RTCRtpCodec {
                    mime_type: MIME_TYPE_OPUS.to_owned(),
                    clock_rate: AUDIO_RATE,
                    channels: 2,
                    sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                    rtcp_feedback: Vec::new(),
                },
                ..Default::default()
            }],
        );
        TrackLocalStaticSample::new(Instant::now(), track)
            .map(Arc::new)
            .map_err(|error| error.to_string())
    }

    fn start_capture(
        &self,
        track: Arc<TrackLocalStaticSample>,
        sample_rate: u32,
        diagnostics: Arc<AudioDiagnostics>,
    ) {
        let capture = self.capture.clone();
        tokio::spawn(async move {
            let frame_samples = (sample_rate.max(1) as usize / 50).max(1);
            let prebuffer_samples = frame_samples.saturating_mul(2);
            while capture.len() < prebuffer_samples {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            let mut encoder = match Encoder::new(AUDIO_RATE, Channels::Mono, Application::Voip) {
                Ok(encoder) => encoder,
                Err(_) => return,
            };
            let _ = encoder.set_bitrate(Bitrate::Bits(24_000));
            let _ = encoder.set_complexity(5);
            let _ = encoder.set_inband_fec(true);
            let _ = encoder.set_packet_loss_perc(5);
            let mut ticker = tokio::time::interval(FRAME_TIME);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut diagnostics_ticker = tokio::time::interval(Duration::from_secs(1));
            // Let the input callback collect a complete frame before the first packet.
            ticker.tick().await;
            diagnostics_ticker.tick().await;
            let mut encoded = [0u8; 400];
            let mut input = Vec::with_capacity((sample_rate.max(1) as usize / 50).max(1));
            loop {
                tokio::select! {
                    _ = diagnostics_ticker.tick() => {
                        report_audio_diagnostics(&diagnostics);
                        continue;
                    }
                    _ = ticker.tick() => {}
                }
                let frame = take_input_frame(&capture, sample_rate, &mut input);
                let size = match encoder.encode_float(&frame, &mut encoded) {
                    Ok(size) => size,
                    Err(_) => continue,
                };
                let sample = MediaSample {
                    data: bytes::Bytes::copy_from_slice(&encoded[..size]),
                    duration: FRAME_TIME,
                    ..MediaSample::new(Instant::now())
                };
                if track
                    .write_sample(AUDIO_SSRC, OPUS_PAYLOAD_TYPE, &sample, &[])
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    async fn apply_pending_candidates(&mut self, peer: &Arc<dyn PeerConnection>) {
        for candidate in std::mem::take(&mut self.pending_candidates) {
            if let Err(error) = peer.add_ice_candidate(candidate).await {
                self.fail(&format!("Could not add a network candidate: {error}"));
                break;
            }
        }
    }

    async fn end(&mut self, notify_remote: bool) {
        self.stop_call_tone();
        if notify_remote && (self.peer.is_some() || self.pending_offer.is_some()) {
            let _ = self.send_signal(json!({"kind":"end"}));
        }
        if let Some(peer) = self.peer.take() {
            let _ = peer.close().await;
        }
        self.audio = None;
        self.pending_offer = None;
        self.pending_candidates.clear();
        while self.capture.pop().is_some() {}
        while self.playback.pop().is_some() {}
        self.peer_failed.store(false, Ordering::Release);
        self.set_state("idle", "Voice chat ready");
    }

    fn send_signal(&self, signal: Value) -> Result<(), String> {
        let mut message = signal;
        let object = message.as_object_mut().ok_or("Invalid voice signal")?;
        object.insert("type".to_owned(), Value::String("signal".to_owned()));
        self.room_sender
            .send(RoomOutbound::Message(message))
            .map_err(|_| "Watch room is disconnected".to_owned())
    }

    fn set_state(&self, state: &str, description: &str) {
        update_ui(&self.window, state, description);
    }

    fn start_incoming_ringtone(&mut self) {
        self.stop_call_tone();
        self.tone_mode.store(TONE_INCOMING, Ordering::Release);
        match open_incoming_ringtone(self.tone_mode.clone()) {
            Ok(stream) => self.incoming_ringtone = Some(stream),
            Err(error) => {
                self.tone_mode.store(TONE_NONE, Ordering::Release);
                eprintln!("Could not play incoming call tone: {error}");
            }
        }
    }

    fn stop_call_tone(&mut self) {
        self.tone_mode.store(TONE_NONE, Ordering::Release);
        self.incoming_ringtone = None;
    }

    fn fail(&mut self, message: &str) {
        self.stop_call_tone();
        self.peer_failed.store(true, Ordering::Release);
        self.set_state("failed", message);
    }
}

fn should_reject_remote_offer(
    peer_exists: bool,
    pending_offer_exists: bool,
    peer_failed: bool,
) -> bool {
    pending_offer_exists || (peer_exists && !peer_failed)
}

#[derive(Deserialize)]
struct IceServerReply {
    urls: Vec<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    credential: Option<String>,
}

#[derive(Deserialize)]
struct IceConfigReply {
    ice_servers: Vec<IceServerReply>,
}

fn fetch_ice_servers(api_base: &str, token: &str) -> Result<Vec<RTCIceServer>, String> {
    let mut response = ureq::get(&format!(
        "{}/api/v1/voice/ice",
        api_base.trim_end_matches('/')
    ))
    .header("Authorization", &format!("Bearer {token}"))
    .call()
    .map_err(|error| format!("Could not load voice network settings: {error}"))?;
    let reply: IceConfigReply = response
        .body_mut()
        .read_json()
        .map_err(|error| format!("Voice network settings were invalid: {error}"))?;
    if reply.ice_servers.is_empty() {
        return Err("The server did not provide any voice network servers.".to_owned());
    }
    Ok(reply
        .ice_servers
        .into_iter()
        .map(|server| RTCIceServer {
            urls: server.urls,
            username: server.username.unwrap_or_default(),
            credential: server.credential.unwrap_or_default(),
        })
        .collect())
}

#[derive(Clone)]
struct VoiceHandler {
    room_sender: Sender<RoomOutbound>,
    playback: Arc<ArrayQueue<f32>>,
    window: slint::Weak<MainWindow>,
    peer_failed: Arc<AtomicBool>,
    tone_mode: Arc<AtomicU8>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for VoiceHandler {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        let Ok(candidate) = event.candidate.to_json() else {
            return;
        };
        if self.room_sender.send(RoomOutbound::Message(json!({
            "type":"signal",
            "kind":"candidate",
            "candidate":candidate.candidate,
            "sdp_mid":candidate.sdp_mid.filter(|mid| !mid.is_empty()).unwrap_or_else(|| "0".to_owned()),
            "sdp_mline_index":candidate.sdp_mline_index.unwrap_or(0),
        }))).is_err() {
            self.peer_failed.store(true, Ordering::Release);
            update_ui(&self.window, "failed", "Watch room disconnected during voice setup.");
        }
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        match state {
            RTCPeerConnectionState::Connected => {
                self.tone_mode.store(TONE_NONE, Ordering::Release);
                update_ui(
                    &self.window,
                    "connected",
                    "Voice connected · peer-to-peer audio",
                )
            }
            RTCPeerConnectionState::Failed => {
                self.tone_mode.store(TONE_NONE, Ordering::Release);
                self.peer_failed.store(true, Ordering::Release);
                update_ui(
                    &self.window,
                    "failed",
                    "Voice connection failed. Check network permissions and try again.",
                );
            }
            RTCPeerConnectionState::Connecting => {
                update_ui(&self.window, "connecting", "Connecting voice…")
            }
            _ => {}
        }
    }

    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        let playback = self.playback.clone();
        tokio::spawn(async move {
            let Ok(mut decoder) = Decoder::new(AUDIO_RATE, Channels::Mono) else {
                return;
            };
            let mut decoded = [0f32; FRAME_SAMPLES * 6];
            while let Some(event) = track.poll().await {
                if let TrackRemoteEvent::OnRtpPacket(packet) = event {
                    let size = match decoder.decode_float(&packet.payload, &mut decoded, false) {
                        Ok(size) => size,
                        Err(_) => continue,
                    };
                    for &sample in &decoded[..size] {
                        if let Err(sample) = playback.push(sample) {
                            let _ = playback.pop();
                            let _ = playback.push(sample);
                        }
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TONE_INCOMING, TONE_NONE, TONE_RINGBACK, call_tone_sample, should_reject_remote_offer,
    };

    #[test]
    fn active_or_pending_calls_reject_another_offer() {
        assert!(should_reject_remote_offer(true, false, false));
        assert!(should_reject_remote_offer(false, true, false));
    }

    #[test]
    fn a_failed_peer_is_replaced_by_the_next_incoming_offer() {
        assert!(!should_reject_remote_offer(true, false, true));
        assert!(!should_reject_remote_offer(false, false, false));
    }

    #[test]
    fn call_tones_have_ring_and_silence_intervals() {
        let rate = 48_000;
        assert_eq!(call_tone_sample(TONE_NONE, rate as u64, rate), 0.0);
        assert!(call_tone_sample(TONE_RINGBACK, 1_000, rate).abs() > 0.02);
        assert_eq!(call_tone_sample(TONE_RINGBACK, rate as u64 * 3, rate), 0.0);
        assert!(call_tone_sample(TONE_INCOMING, 1_000, rate).abs() > 0.02);
        assert_eq!(call_tone_sample(TONE_INCOMING, rate as u64 / 2, rate), 0.0);
        assert!(call_tone_sample(TONE_INCOMING, rate as u64 * 7 / 10 + 1_000, rate).abs() > 0.02);
        assert_eq!(
            call_tone_sample(TONE_INCOMING, rate as u64 * 3 / 2, rate),
            0.0
        );
    }
}

struct AudioIo {
    _input: cpal::Stream,
    _output: cpal::Stream,
    input_rate: u32,
    diagnostics: Arc<AudioDiagnostics>,
}

#[derive(Default)]
struct AudioDiagnostics {
    input_xruns: AtomicU64,
    output_xruns: AtomicU64,
    input_errors: AtomicU64,
    output_errors: AtomicU64,
}

fn report_audio_diagnostics(diagnostics: &AudioDiagnostics) {
    let input_xruns = diagnostics.input_xruns.swap(0, Ordering::Relaxed);
    if input_xruns > 0 {
        eprintln!("Microphone capture reported {input_xruns} audio discontinuity event(s).");
    }
    let output_xruns = diagnostics.output_xruns.swap(0, Ordering::Relaxed);
    if output_xruns > 0 {
        eprintln!("Speaker playback reported {output_xruns} audio discontinuity event(s).");
    }
    let input_errors = diagnostics.input_errors.swap(0, Ordering::Relaxed);
    if input_errors > 0 {
        eprintln!("Microphone stream reported {input_errors} additional audio error event(s).");
    }
    let output_errors = diagnostics.output_errors.swap(0, Ordering::Relaxed);
    if output_errors > 0 {
        eprintln!("Speaker stream reported {output_errors} additional audio error event(s).");
    }
}

fn open_audio(
    capture: Arc<ArrayQueue<f32>>,
    playback: Arc<ArrayQueue<f32>>,
    tone_mode: Arc<AtomicU8>,
) -> Result<AudioIo, String> {
    let host = cpal::default_host();
    let input_device = host
        .default_input_device()
        .ok_or("No microphone is available")?;
    let output_device = host
        .default_output_device()
        .ok_or("No speaker or headset is available")?;
    let input = input_device
        .default_input_config()
        .map_err(|error| error.to_string())?;
    let output = output_device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    let input_rate = input.sample_rate();
    let diagnostics = Arc::new(AudioDiagnostics::default());
    let input_stream = build_input(&input_device, input, capture.clone(), diagnostics.clone())?;
    let output_stream = build_output(
        &output_device,
        output,
        playback,
        tone_mode,
        diagnostics.clone(),
    )?;
    input_stream.play().map_err(|error| error.to_string())?;
    output_stream.play().map_err(|error| error.to_string())?;
    Ok(AudioIo {
        _input: input_stream,
        _output: output_stream,
        input_rate,
        diagnostics,
    })
}

fn open_incoming_ringtone(tone_mode: Arc<AtomicU8>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let output_device = host
        .default_output_device()
        .ok_or("No speaker or headset is available")?;
    let output = output_device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    let playback = Arc::new(ArrayQueue::new(1));
    let diagnostics = Arc::new(AudioDiagnostics::default());
    let stream = build_output(&output_device, output, playback, tone_mode, diagnostics)?;
    stream.play().map_err(|error| error.to_string())?;
    Ok(stream)
}

fn build_input(
    device: &cpal::Device,
    config: cpal::SupportedStreamConfig,
    capture: Arc<ArrayQueue<f32>>,
    diagnostics: Arc<AudioDiagnostics>,
) -> Result<cpal::Stream, String> {
    let channels = config.channels() as usize;
    // Keep CPAL's host-selected period and buffer sizing. CPAL is built with
    // realtime scheduling support so eligible audio callback threads can be
    // promoted instead of relying on a larger, higher-latency fixed period.
    let stream_config = stable_stream_config(&config);
    let data_capture = capture;
    let error = move |error: cpal::Error| {
        let counter = if error.kind() == cpal::ErrorKind::Xrun {
            &diagnostics.input_xruns
        } else {
            &diagnostics.input_errors
        };
        counter.fetch_add(1, Ordering::Relaxed);
    };
    match config.sample_format() {
        SampleFormat::F32 => {
            input_stream::<f32>(device, stream_config, channels, data_capture, error)
        }
        SampleFormat::I16 => {
            input_stream::<i16>(device, stream_config, channels, data_capture, error)
        }
        SampleFormat::U16 => {
            input_stream::<u16>(device, stream_config, channels, data_capture, error)
        }
        SampleFormat::I32 => {
            input_stream::<i32>(device, stream_config, channels, data_capture, error)
        }
        SampleFormat::U32 => {
            input_stream::<u32>(device, stream_config, channels, data_capture, error)
        }
        SampleFormat::F64 => {
            input_stream::<f64>(device, stream_config, channels, data_capture, error)
        }
        format => Err(format!("Unsupported microphone sample format: {format}")),
    }
}

fn input_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    capture: Arc<ArrayQueue<f32>>,
    error: impl Fn(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Sample + Copy,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                for frame in data.chunks(channels) {
                    let mono = frame
                        .iter()
                        .map(|sample| f32::from_sample(*sample))
                        .sum::<f32>()
                        / channels as f32;
                    if let Err(mono) = capture.push(mono.clamp(-1.0, 1.0)) {
                        let _ = capture.pop();
                        let _ = capture.push(mono);
                    }
                }
            },
            error,
            None,
        )
        .map_err(|error| error.to_string())
}

fn build_output(
    device: &cpal::Device,
    config: cpal::SupportedStreamConfig,
    playback: Arc<ArrayQueue<f32>>,
    tone_mode: Arc<AtomicU8>,
    diagnostics: Arc<AudioDiagnostics>,
) -> Result<cpal::Stream, String> {
    let channels = config.channels() as usize;
    let rate = config.sample_rate();
    let stream_config = stable_stream_config(&config);
    let error = move |error: cpal::Error| {
        let counter = if error.kind() == cpal::ErrorKind::Xrun {
            &diagnostics.output_xruns
        } else {
            &diagnostics.output_errors
        };
        counter.fetch_add(1, Ordering::Relaxed);
    };
    match config.sample_format() {
        SampleFormat::F32 => output_stream::<f32>(
            device,
            stream_config,
            channels,
            rate,
            playback,
            tone_mode,
            error,
        ),
        SampleFormat::I16 => output_stream::<i16>(
            device,
            stream_config,
            channels,
            rate,
            playback,
            tone_mode,
            error,
        ),
        SampleFormat::U16 => output_stream::<u16>(
            device,
            stream_config,
            channels,
            rate,
            playback,
            tone_mode,
            error,
        ),
        SampleFormat::I32 => output_stream::<i32>(
            device,
            stream_config,
            channels,
            rate,
            playback,
            tone_mode,
            error,
        ),
        SampleFormat::U32 => output_stream::<u32>(
            device,
            stream_config,
            channels,
            rate,
            playback,
            tone_mode,
            error,
        ),
        SampleFormat::F64 => output_stream::<f64>(
            device,
            stream_config,
            channels,
            rate,
            playback,
            tone_mode,
            error,
        ),
        format => Err(format!("Unsupported speaker sample format: {format}")),
    }
}

fn output_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    sample_rate: u32,
    playback: Arc<ArrayQueue<f32>>,
    tone_mode: Arc<AtomicU8>,
    error: impl Fn(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Sample + FromSample<f32>,
{
    let step = AUDIO_RATE as f64 / sample_rate.max(1) as f64;
    let mut fractional = 0.0f64;
    let mut previous = 0.0f32;
    let mut next = 0.0f32;
    let mut output_frame = 0u64;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for frame in data.chunks_mut(channels) {
                    let voice = previous + ((next - previous) * fractional as f32);
                    fractional += step;
                    while fractional >= 1.0 {
                        previous = next;
                        next = playback.pop().unwrap_or(0.0);
                        fractional -= 1.0;
                    }
                    let tone = call_tone_sample(
                        tone_mode.load(Ordering::Relaxed),
                        output_frame,
                        sample_rate,
                    );
                    let value = (voice + tone).clamp(-0.98, 0.98);
                    output_frame = output_frame.wrapping_add(1);
                    for sample in frame {
                        *sample = T::from_sample(value);
                    }
                }
            },
            error,
            None,
        )
        .map_err(|error| error.to_string())
}

fn call_tone_sample(mode: u8, frame: u64, sample_rate: u32) -> f32 {
    if mode == TONE_NONE || sample_rate == 0 {
        return 0.0;
    }
    let sample_rate = sample_rate as u64;
    let (cycle, duration, frequencies): (u64, u64, &[f64]) = match mode {
        TONE_RINGBACK => (
            sample_rate.saturating_mul(6),
            sample_rate.saturating_mul(2),
            &[440.0, 480.0],
        ),
        TONE_INCOMING => {
            let cycle_position = frame % (sample_rate.saturating_mul(5) / 2).max(1);
            let first_burst = sample_rate.saturating_mul(400) / 1_000;
            let second_start = sample_rate.saturating_mul(600) / 1_000;
            let second_end = sample_rate;
            if cycle_position < first_burst || (second_start..second_end).contains(&cycle_position)
            {
                let position = if cycle_position < first_burst {
                    cycle_position
                } else {
                    cycle_position - second_start
                };
                return tone_sample(position, first_burst, &[440.0, 480.0], sample_rate);
            }
            return 0.0;
        }
        _ => return 0.0,
    };
    tone_sample(frame % cycle.max(1), duration, frequencies, sample_rate)
}

fn tone_sample(position: u64, duration: u64, frequencies: &[f64], sample_rate: u64) -> f32 {
    if position >= duration || sample_rate == 0 {
        return 0.0;
    }
    let fade = (sample_rate.saturating_mul(8) / 1_000).max(1);
    let remaining = duration - position;
    let envelope = (position as f32 / fade as f32)
        .min(1.0)
        .min(remaining as f32 / fade as f32);
    let elapsed = position as f64 / sample_rate as f64;
    let tone = frequencies
        .iter()
        .map(|frequency| (std::f64::consts::TAU * frequency * elapsed).sin())
        .sum::<f64>() as f32
        / frequencies.len().max(1) as f32;
    tone * envelope * 0.16
}

fn stable_stream_config(config: &cpal::SupportedStreamConfig) -> cpal::StreamConfig {
    // Retain the host's supported timing; fixed periods have caused XRUNs on
    // shared-mode WASAPI and PipeWire-backed ALSA devices.
    config.config()
}

fn take_input_frame(
    capture: &Arc<ArrayQueue<f32>>,
    source_rate: u32,
    input: &mut Vec<f32>,
) -> [f32; FRAME_SAMPLES] {
    let wanted = (source_rate.max(1) as usize / 50).max(1);
    // Keep a short input cushion during normal device callback jitter, but trim a
    // long backlog after scheduler stalls so the call does not accumulate latency.
    let max_backlog = (source_rate.max(1) as usize / 10).max(wanted.saturating_mul(2));
    let stale = capture.len().saturating_sub(max_backlog);
    for _ in 0..stale {
        let _ = capture.pop();
    }
    input.clear();
    for _ in 0..wanted {
        input.push(capture.pop().unwrap_or(0.0));
    }
    if input.len() < wanted {
        input.resize(wanted, 0.0);
    }
    if wanted == FRAME_SAMPLES {
        let mut output = [0.0f32; FRAME_SAMPLES];
        output.copy_from_slice(input);
        return output;
    }
    let mut output = [0.0f32; FRAME_SAMPLES];
    for (index, sample) in output.iter_mut().enumerate() {
        let position =
            index as f64 * (input.len().saturating_sub(1)) as f64 / (FRAME_SAMPLES - 1) as f64;
        let left = position.floor() as usize;
        let right = (left + 1).min(input.len() - 1);
        let fraction = (position - left as f64) as f32;
        *sample = input[left] + (input[right] - input[left]) * fraction;
    }
    output
}

fn update_ui(window: &slint::Weak<MainWindow>, state: &str, description: &str) {
    let window = window.clone();
    let state = state.to_owned();
    let description = description.to_owned();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            ui.set_voice_state(state.into());
            ui.set_voice_description(description.into());
        }
    });
}
