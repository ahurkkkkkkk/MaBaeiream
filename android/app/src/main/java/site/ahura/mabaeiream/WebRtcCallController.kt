package site.ahura.mabaeiream

import android.content.Context
import android.os.Handler
import android.os.Looper
import org.webrtc.AudioSource
import org.webrtc.AudioTrack
import org.webrtc.DataChannel
import org.webrtc.IceCandidate
import org.webrtc.audio.JavaAudioDeviceModule
import org.webrtc.MediaConstraints
import org.webrtc.MediaStream
import org.webrtc.PeerConnection
import org.webrtc.PeerConnectionFactory
import org.webrtc.RtpReceiver
import org.webrtc.RtpTransceiver
import org.webrtc.SdpObserver
import org.webrtc.SessionDescription
import org.webrtc.VideoTrack

internal sealed interface VoiceCallState {
    data object Idle : VoiceCallState
    data class Incoming(val from: String) : VoiceCallState
    data object Calling : VoiceCallState
    data object Connecting : VoiceCallState
    data object Connected : VoiceCallState
    data class Failed(val message: String) : VoiceCallState
}

/** Foreground-only, audio-only WebRTC call. Signaling stays on the authenticated room socket. */
internal class WebRtcCallController(
    context: Context,
    private val voiceIceServers: List<VoiceIceServer>,
    private val sendOffer: (String) -> Boolean,
    private val sendAnswer: (String) -> Boolean,
    private val sendCandidate: (String, String?, Int?) -> Boolean,
    private val sendEnd: () -> Boolean,
    private val onState: (VoiceCallState) -> Unit,
    private val onSpeakerphoneChanged: (Boolean) -> Unit,
) : AutoCloseable {
    private val appContext = context.applicationContext
    private val mainHandler = Handler(Looper.getMainLooper())
    private val callAudio = VoiceCallAudio(appContext, onSpeakerphoneChanged)
    private var factory: PeerConnectionFactory? = null
    private var audioDeviceModule: JavaAudioDeviceModule? = null
    private var peer: PeerConnection? = null
    private var localSource: AudioSource? = null
    private var localAudioTrack: AudioTrack? = null
    private var pendingOffer: RoomSignal.CallOffer? = null
    private val pendingCandidates = mutableListOf<IceCandidate>()
    private var remoteDescriptionReady = false
    // WebRTC callbacks run on its signaling thread while room messages arrive
    // on the Compose/main thread. Keep candidate state serialized; racing these
    // callbacks can strand the call in Connecting on slower Android devices.
    private val signalingLock = Any()
    private var closed = false
    @Volatile private var state: VoiceCallState = VoiceCallState.Idle

    fun startOutgoing() {
        if (closed || state !is VoiceCallState.Idle) return
        if (!createPeer()) return
        update(VoiceCallState.Calling)
        val connection = peer ?: return
        protectNative("Could not create a voice call.") {
            connection.createOffer(object : SdpObserver {
            override fun onCreateSuccess(description: SessionDescription) = protectNative("Could not prepare the call offer.") {
                connection.setLocalDescription(object : SdpObserver {
                    override fun onSetSuccess() = protectNative("Could not send the call offer.") {
                        if (!sendOffer(description.description)) {
                            fail("Could not send the call offer.")
                        } else {
                            callAudio.playOutgoingRingback()
                        }
                    }
                    override fun onCreateSuccess(description: SessionDescription?) = Unit
                    override fun onCreateFailure(error: String?) = fail(error ?: "Could not create the call offer.")
                    override fun onSetFailure(error: String?) = fail(error ?: "Could not set the local call offer.")
                }, description)
            }
            override fun onSetSuccess() = Unit
            override fun onCreateFailure(error: String?) = fail(error ?: "Could not create a call.")
            override fun onSetFailure(error: String?) = fail(error ?: "Could not create a call.")
            }, audioConstraints())
        }
    }

    fun handleSignal(signal: RoomSignal) {
        if (closed) return
        protectNative("Could not update the voice call.") {
        when (signal) {
            is RoomSignal.CallOffer -> {
                if (state is VoiceCallState.Idle) {
                    pendingOffer = signal
                    callAudio.playIncomingRingtone()
                    update(VoiceCallState.Incoming(signal.from))
                } else if (signal.from != "") {
                    sendEnd()
                }
            }
            is RoomSignal.CallAnswer -> {
                callAudio.stopTone()
                val connection = peer ?: return
                connection.setRemoteDescription(remoteDescriptionObserver(), SessionDescription(SessionDescription.Type.ANSWER, signal.sdp))
            }
            is RoomSignal.IceCandidate -> {
                val candidate = IceCandidate(signal.sdpMid, signal.sdpMLineIndex ?: 0, signal.candidate)
                val connection = synchronized(signalingLock) {
                    val current = peer
                    if (current == null || !remoteDescriptionReady) {
                        pendingCandidates += candidate
                        null
                    } else {
                        current
                    }
                }
                connection?.let { it.addIceCandidate(candidate) }
            }
            is RoomSignal.CallEnded -> {
                val wasWaiting = state is VoiceCallState.Calling || state is VoiceCallState.Connecting
                closePeer(
                    if (wasWaiting) VoiceCallState.Failed("The other device ended the call before it connected.")
                    else VoiceCallState.Idle,
                )
            }
        }
        }
    }

    fun acceptIncoming() {
        val offer = pendingOffer ?: return
        if (closed || state !is VoiceCallState.Incoming) return
        callAudio.stopTone()
        if (!createPeer()) return
        val connection = peer ?: return
        protectNative("Could not accept the incoming call.") {
        connection.setRemoteDescription(object : SdpObserver {
            override fun onSetSuccess() = protectNative("Could not prepare the call answer.") {
                markRemoteDescriptionReady(connection)
                connection.createAnswer(object : SdpObserver {
                    override fun onCreateSuccess(description: SessionDescription) = protectNative("Could not prepare the call answer.") {
                        connection.setLocalDescription(object : SdpObserver {
                            override fun onSetSuccess() = protectNative("Could not send the call answer.") {
                                if (!sendAnswer(description.description)) fail("Could not send the call answer.")
                            }
                            override fun onCreateSuccess(description: SessionDescription?) = Unit
                            override fun onCreateFailure(error: String?) = fail(error ?: "Could not create the call answer.")
                            override fun onSetFailure(error: String?) = fail(error ?: "Could not set the local call answer.")
                        }, description)
                    }
                    override fun onSetSuccess() = Unit
                    override fun onCreateFailure(error: String?) = fail(error ?: "Could not create the call answer.")
                    override fun onSetFailure(error: String?) = fail(error ?: "Could not create the call answer.")
                }, audioConstraints())
            }
            override fun onCreateSuccess(description: SessionDescription?) = Unit
            override fun onCreateFailure(error: String?) = fail(error ?: "Could not read the incoming call offer.")
            override fun onSetFailure(error: String?) = fail(error ?: "Could not accept the incoming call.")
        }, SessionDescription(SessionDescription.Type.OFFER, offer.sdp))
        }
        pendingOffer = null
    }

    fun declineIncoming() {
        pendingOffer = null
        sendEnd()
        closePeer(VoiceCallState.Idle)
    }

    fun hangUp() {
        if (state !is VoiceCallState.Idle) sendEnd()
        pendingOffer = null
        closePeer(VoiceCallState.Idle)
    }

    /** A transient system overlay can briefly stop the Activity during call setup. */
    fun endAfterBackgroundTimeout() {
        if (closed || state is VoiceCallState.Idle) return
        sendEnd()
        pendingOffer = null
        closePeer(
            VoiceCallState.Failed(
                "Voice chat ended because MaBaeiream stayed in the background. Keep the app open and try again.",
            ),
        )
    }

    fun dismissError() {
        if (state is VoiceCallState.Failed) closePeer(VoiceCallState.Idle)
    }

    fun toggleSpeakerphone() {
        callAudio.toggleSpeakerphone()
    }

    private fun createPeer(): Boolean {
        if (peer != null) return true
        return try {
            callAudio.beginCommunication()
            val peerFactory = ensureFactory()
            callAudio.reassertDefaultSpeakerphone()
            val configuration = PeerConnection.RTCConfiguration(
                voiceIceServers.flatMap { server ->
                    server.urls.map { url ->
                        PeerConnection.IceServer.builder(url).apply {
                            server.username?.let(::setUsername)
                            server.credential?.let(::setPassword)
                        }.createIceServer()
                    }
                },
            ).apply {
                sdpSemantics = PeerConnection.SdpSemantics.UNIFIED_PLAN
                bundlePolicy = PeerConnection.BundlePolicy.MAXBUNDLE
                rtcpMuxPolicy = PeerConnection.RtcpMuxPolicy.REQUIRE
            }
            val connection = peerFactory.createPeerConnection(configuration, object : PeerConnection.Observer {
                override fun onSignalingChange(state: PeerConnection.SignalingState) = Unit
                override fun onIceConnectionChange(state: PeerConnection.IceConnectionState) {
                    if (state == PeerConnection.IceConnectionState.CONNECTED || state == PeerConnection.IceConnectionState.COMPLETED) {
                        callAudio.stopTone()
                        reassertDefaultSpeakerphoneOnMain()
                        update(VoiceCallState.Connected)
                    } else if (state == PeerConnection.IceConnectionState.FAILED) {
                        fail("The call could not reach the other device. Check both networks and try again.")
                    }
                }
                override fun onIceConnectionReceivingChange(receiving: Boolean) = Unit
                override fun onIceGatheringChange(state: PeerConnection.IceGatheringState) = Unit
                override fun onIceCandidate(candidate: IceCandidate) {
                    if (!sendCandidate(candidate.sdp, candidate.sdpMid, candidate.sdpMLineIndex)) {
                        fail("Could not send an ICE network candidate.")
                    }
                }
                override fun onIceCandidatesRemoved(candidates: Array<out IceCandidate>) = Unit
                override fun onAddStream(stream: MediaStream) = Unit
                override fun onRemoveStream(stream: MediaStream) = Unit
                override fun onDataChannel(channel: DataChannel) = Unit
                override fun onRenegotiationNeeded() = Unit
                override fun onAddTrack(receiver: RtpReceiver, mediaStreams: Array<out MediaStream>) = Unit
                override fun onTrack(transceiver: RtpTransceiver) {
                    (transceiver.receiver.track() as? AudioTrack)?.apply {
                        setEnabled(true)
                        // A modest receive-side boost helps voices stay clear on
                        // phone speakers without changing microphone gain.
                        setVolume(2.0)
                    }
                }
                override fun onConnectionChange(newState: PeerConnection.PeerConnectionState) {
                    if (newState == PeerConnection.PeerConnectionState.CONNECTED) {
                        callAudio.stopTone()
                        reassertDefaultSpeakerphoneOnMain()
                        update(VoiceCallState.Connected)
                    }
                    if (newState == PeerConnection.PeerConnectionState.FAILED) {
                        fail("The call could not reach the other device. Check both networks and try again.")
                    }
                }
            }) ?: error("Could not initialize the WebRTC audio connection.")
            peer = connection
            val source = peerFactory.createAudioSource(audioConstraints())
            val track = peerFactory.createAudioTrack("mabaeiream-microphone", source).also { it.setEnabled(true) }
            localSource = source
            localAudioTrack = track
            connection.addTrack(track, listOf("mabaeiream-audio"))
            callAudio.reassertDefaultSpeakerphone()
            update(VoiceCallState.Connecting)
            true
        } catch (error: Exception) {
            fail(error.message ?: "Could not start WebRTC voice chat.")
            false
        } catch (error: LinkageError) {
            fail("The voice calling component is unavailable in this app build.")
            false
        }
    }

    /** Delay native audio setup until the user has granted microphone access and starts a call. */
    private fun ensureFactory(): PeerConnectionFactory {
        factory?.let { return it }
        WebRtcRuntime.ensureInitialized(appContext)
        val deviceModule = JavaAudioDeviceModule.builder(appContext)
            // Device-provided effects vary widely; libwebrtc's built-in processing is more reliable.
            .setUseHardwareAcousticEchoCanceler(false)
            .setUseHardwareNoiseSuppressor(false)
            .createAudioDeviceModule()
        return try {
            val created = PeerConnectionFactory.builder()
                .setAudioDeviceModule(deviceModule)
                .createPeerConnectionFactory()
            audioDeviceModule = deviceModule
            factory = created
            created
        } catch (error: Exception) {
            deviceModule.release()
            throw error
        } catch (error: LinkageError) {
            deviceModule.release()
            throw error
        }
    }

    private fun remoteDescriptionObserver() = object : SdpObserver {
        override fun onSetSuccess() {
            peer?.let(::markRemoteDescriptionReady)
        }
        override fun onCreateSuccess(description: SessionDescription?) = Unit
        override fun onCreateFailure(error: String?) = fail(error ?: "Could not read the call answer.")
        override fun onSetFailure(error: String?) = fail(error ?: "Could not apply the call answer.")
    }

    private fun markRemoteDescriptionReady(connection: PeerConnection) {
        val waiting = synchronized(signalingLock) {
            remoteDescriptionReady = true
            pendingCandidates.toList().also { pendingCandidates.clear() }
        }
        protectNative("Could not add the call's network candidate.") {
            waiting.forEach(connection::addIceCandidate)
        }
        update(VoiceCallState.Connecting)
    }

    private inline fun protectNative(fallback: String, operation: () -> Unit) {
        try {
            operation()
        } catch (error: Exception) {
            fail(error.message?.takeIf { it.isNotBlank() } ?: fallback)
        } catch (error: LinkageError) {
            fail("The voice calling component is unavailable in this app build.")
        }
    }

    private inline fun safelyRelease(operation: () -> Unit) {
        try {
            operation()
        } catch (_: Exception) {
        } catch (_: LinkageError) {
        }
    }

    private fun audioConstraints() = MediaConstraints().apply {
        mandatory.add(MediaConstraints.KeyValuePair("googEchoCancellation", "true"))
        mandatory.add(MediaConstraints.KeyValuePair("googNoiseSuppression", "true"))
        mandatory.add(MediaConstraints.KeyValuePair("googAutoGainControl", "true"))
        mandatory.add(MediaConstraints.KeyValuePair("googHighpassFilter", "true"))
    }

    private fun fail(message: String) {
        closePeer(VoiceCallState.Failed(message))
    }

    private fun closePeer(next: VoiceCallState) {
        pendingOffer = null
        callAudio.endCommunication()
        val connection = synchronized(signalingLock) {
            val current = peer
            peer = null
            remoteDescriptionReady = false
            pendingCandidates.clear()
            current
        }
        connection?.let {
            safelyRelease { connection.close() }
            safelyRelease { connection.dispose() }
        }
        localAudioTrack?.let { safelyRelease { it.dispose() } }
        localAudioTrack = null
        localSource?.let { safelyRelease { it.dispose() } }
        localSource = null
        update(next)
    }

    private fun update(next: VoiceCallState) {
        state = next
        mainHandler.post { onState(next) }
    }

    private fun reassertDefaultSpeakerphoneOnMain() {
        mainHandler.post { callAudio.reassertDefaultSpeakerphone() }
    }

    override fun close() {
        if (closed) return
        closed = true
        closePeer(VoiceCallState.Idle)
        callAudio.close()
        factory?.let { safelyRelease { it.dispose() } }
        factory = null
        audioDeviceModule?.let { safelyRelease { it.release() } }
        audioDeviceModule = null
    }
}

private object WebRtcRuntime {
    @Volatile private var initialized = false

    @Synchronized
    fun ensureInitialized(context: Context) {
        if (initialized) return
        PeerConnectionFactory.initialize(
            PeerConnectionFactory.InitializationOptions.builder(context)
                .createInitializationOptions(),
        )
        initialized = true
    }
}
