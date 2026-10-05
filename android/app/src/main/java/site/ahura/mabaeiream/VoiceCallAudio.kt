package site.ahura.mabaeiream

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.media.Ringtone
import android.media.RingtoneManager
import android.media.ToneGenerator
import android.os.Build
import android.os.Handler
import android.os.Looper

/** Call routing and call-progress sounds, kept out of WebRTC's media callbacks. */
internal class VoiceCallAudio(
    context: Context,
    private val onSpeakerphoneChanged: (Boolean) -> Unit,
) : AutoCloseable {
    private val appContext = context.applicationContext
    private val audioManager = appContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private val mainHandler = Handler(Looper.getMainLooper())
    private var previousMode: Int? = null
    private var previousSpeakerphone = false
    private var speakerphone = false
    private var userSelectedRoute = false
    private var ringtone: Ringtone? = null
    private var toneGenerator: ToneGenerator? = null
    private var toneLoop: Runnable? = null

    fun beginCommunication() {
        if (previousMode != null) return
        previousMode = audioManager.mode
        previousSpeakerphone = audioManager.isSpeakerphoneOn
        userSelectedRoute = false
        audioManager.mode = AudioManager.MODE_IN_COMMUNICATION
        routeToSpeakerphone()
    }

    /** Re-apply the default after WebRTC initializes its Android audio device module. */
    fun reassertDefaultSpeakerphone() {
        if (previousMode == null || userSelectedRoute) return
        routeToSpeakerphone()
    }

    fun toggleSpeakerphone() {
        if (previousMode == null) return
        userSelectedRoute = true
        if (speakerphone) {
            routeToPrivateAudio()
        } else {
            routeToSpeakerphone()
        }
    }

    fun playIncomingRingtone() {
        stopTone()
        if (audioManager.ringerMode == AudioManager.RINGER_MODE_SILENT) return
        val uri = RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE)
        val defaultRingtone = uri?.let { runCatching { RingtoneManager.getRingtone(appContext, it) }.getOrNull() }
        if (defaultRingtone != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            defaultRingtone.isLooping = true
            ringtone = defaultRingtone
            runCatching { defaultRingtone.play() }
        } else {
            startRepeatingTone(AudioManager.STREAM_RING, 38)
        }
    }

    fun playOutgoingRingback() {
        stopTone()
        startRepeatingTone(AudioManager.STREAM_VOICE_CALL, 30)
    }

    fun stopTone() {
        toneLoop?.let(mainHandler::removeCallbacks)
        toneLoop = null
        ringtone?.let { runCatching { it.stop() } }
        ringtone = null
        toneGenerator?.let {
            runCatching { it.stopTone() }
            runCatching { it.release() }
        }
        toneGenerator = null
    }

    fun endCommunication() {
        stopTone()
        val mode = previousMode ?: return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            runCatching { audioManager.clearCommunicationDevice() }
        } else {
            runCatching { audioManager.isSpeakerphoneOn = previousSpeakerphone }
        }
        runCatching { audioManager.mode = mode }
        previousMode = null
        userSelectedRoute = false
        setSpeakerphoneState(false)
    }

    private fun routeToSpeakerphone() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val speaker = audioManager.availableCommunicationDevices
                .firstOrNull { it.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER }
            if (speaker != null && audioManager.setCommunicationDevice(speaker)) {
                setSpeakerphoneState(true)
                return
            }
        }
        setLegacySpeakerphone(true)
    }

    private fun routeToPrivateAudio() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val devices = audioManager.availableCommunicationDevices
            val device = preferredHeadset(devices)
                ?: devices.firstOrNull { it.type == AudioDeviceInfo.TYPE_BUILTIN_EARPIECE }
            if (device != null && audioManager.setCommunicationDevice(device)) {
                setSpeakerphoneState(false)
                return
            }
        }
        setLegacySpeakerphone(false)
    }

    private fun startRepeatingTone(stream: Int, volume: Int) {
        val generator = runCatching { ToneGenerator(stream, volume) }.getOrNull() ?: return
        toneGenerator = generator
        val loop = object : Runnable {
            override fun run() {
                if (toneGenerator !== generator) return
                runCatching { generator.startTone(ToneGenerator.TONE_SUP_RINGTONE) }
                mainHandler.postDelayed(this, RINGBACK_INTERVAL_MS)
            }
        }
        toneLoop = loop
        loop.run()
    }

    private fun preferredHeadset(devices: List<AudioDeviceInfo>): AudioDeviceInfo? = devices.firstOrNull {
        it.type == AudioDeviceInfo.TYPE_BLUETOOTH_SCO ||
            it.type == AudioDeviceInfo.TYPE_BLE_HEADSET ||
            it.type == AudioDeviceInfo.TYPE_WIRED_HEADSET ||
            it.type == AudioDeviceInfo.TYPE_WIRED_HEADPHONES ||
            it.type == AudioDeviceInfo.TYPE_USB_HEADSET
    }

    @Suppress("DEPRECATION")
    private fun setLegacySpeakerphone(enabled: Boolean) {
        runCatching { audioManager.isSpeakerphoneOn = enabled }
        setSpeakerphoneState(enabled)
    }

    private fun setSpeakerphoneState(enabled: Boolean) {
        speakerphone = enabled
        onSpeakerphoneChanged(enabled)
    }

    override fun close() {
        endCommunication()
    }

    private companion object {
        const val RINGBACK_INTERVAL_MS = 5_500L
    }
}
