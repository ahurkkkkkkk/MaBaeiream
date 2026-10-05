package site.ahura.mabaeiream

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import java.io.Closeable
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URI
import java.net.URLEncoder
import java.net.URL
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import okhttp3.HttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull

data class MediaEntry(val name: String, val path: String, val kind: String, val size: Long)
data class SubtitleEntry(val name: String, val path: String, val mimeType: String)
data class VoiceIceServer(val urls: List<String>, val username: String?, val credential: String?)
data class ResolvedMedia(
    val url: String,
    val audioUrl: String?,
    val title: String,
    val headers: Map<String, String>,
)
data class LoginResult(val token: String, val username: String)
data class DownloadStatus(val state: String, val bytes: Long, val total: Long?, val savedAs: String?, val error: String?)
data class UploadedMedia(val name: String, val path: String, val size: Long)
data class LiveConfig(
    val streamPath: String,
    val whipUrl: String,
    val rtmpsServerUrl: String,
    val streamKey: String,
)

data class RoomState(
    val revision: Long,
    val sourceKind: String?,
    val source: String?,
    val title: String?,
    val playing: Boolean,
    val positionMs: Long,
    val serverTimeMs: Long,
    val updatedBy: String?,
    val receivedElapsedMs: Long = 0L,
)

sealed interface RoomSignal {
    val from: String

    data class CallOffer(override val from: String, val sdp: String) : RoomSignal

    data class CallAnswer(override val from: String, val sdp: String) : RoomSignal

    data class IceCandidate(
        override val from: String,
        val candidate: String,
        val sdpMid: String?,
        val sdpMLineIndex: Int?,
    ) : RoomSignal

    data class CallEnded(override val from: String) : RoomSignal
}

/** Authenticated connection to MaBaeiream's single shared playback room. */
class RoomConnection private constructor(
    private val webSocket: WebSocket,
    private val clientClosed: java.util.concurrent.atomic.AtomicBoolean,
    private val outbound: RoomSocketOutbox,
) : Closeable {
    /** Send a library path or HTTPS URL selection to every connected room client. */
    fun select(sourceKind: String, source: String, title: String, playing: Boolean = true): Boolean {
        require(sourceKind == SOURCE_KIND_LIBRARY || sourceKind == SOURCE_KIND_HTTPS || sourceKind == SOURCE_KIND_LIVE) {
            "Room source kind must be 'library', 'https', or 'live'."
        }
        return send(
            JSONObject()
                .put("type", "select")
                .put("source_kind", sourceKind)
                .put("source", source)
                .put("title", title)
                .put("playing", playing),
        )
    }

    fun play(positionMs: Long): Boolean = sendPlaybackCommand("play", positionMs)

    fun pause(positionMs: Long): Boolean = sendPlaybackCommand("pause", positionMs)

    fun seek(positionMs: Long): Boolean = sendPlaybackCommand("seek", positionMs)

    fun sendCallOffer(sdp: String): Boolean {
        require(sdp.startsWith("v=0") && sdp.length <= 32 * 1024) {
            "The WebRTC offer is invalid or too large."
        }
        return sendSignal("offer", "sdp", sdp)
    }

    fun sendCallAnswer(sdp: String): Boolean {
        require(sdp.startsWith("v=0") && sdp.length <= 32 * 1024) {
            "The WebRTC answer is invalid or too large."
        }
        return sendSignal("answer", "sdp", sdp)
    }

    fun sendIceCandidate(candidate: String, sdpMid: String?, sdpMLineIndex: Int?): Boolean {
        require(candidate.startsWith("candidate:") && candidate.length <= 2048) {
            "The WebRTC ICE candidate is invalid or too large."
        }
        val message = JSONObject()
            .put("type", "signal")
            .put("kind", "candidate")
            .put("candidate", candidate)
        sdpMid?.let { message.put("sdp_mid", it) }
        sdpMLineIndex?.let { message.put("sdp_mline_index", it) }
        return send(message)
    }

    fun sendCallEnd(): Boolean {
        return sendSignal("end")
    }

    private fun sendSignal(kind: String, field: String? = null, value: String? = null): Boolean {
        val message = JSONObject().put("type", "signal").put("kind", kind)
        if (field != null && value != null) message.put(field, value)
        return send(message)
    }

    private fun sendPlaybackCommand(type: String, positionMs: Long): Boolean {
        require(positionMs >= 0L) { "Playback position cannot be negative." }
        return send(JSONObject().put("type", type).put("position_ms", positionMs))
    }

    private fun send(message: JSONObject): Boolean = outbound.send(webSocket, message.toString())

    companion object {
        private const val SOURCE_KIND_LIBRARY = "library"
        private const val SOURCE_KIND_HTTPS = "https"
        private const val SOURCE_KIND_LIVE = "live"
        private const val NORMAL_CLOSE = 1000

        private val mainHandler = Handler(Looper.getMainLooper())
        private val client: OkHttpClient by lazy {
            OkHttpClient.Builder()
                .pingInterval(30, TimeUnit.SECONDS)
                .build()
        }

        /**
         * Opens the secure WebSocket at `<base>/api/v1/room/ws` with the bearer
         * token in the Authorization header. OkHttp represents WSS endpoints as
         * HTTPS URLs and performs the WebSocket upgrade over TLS. Callbacks are
         * dispatched on Android's main thread.
         */
        fun connect(
            baseUrl: String,
            token: String,
            onState: (RoomState) -> Unit,
            onError: (String) -> Unit,
            onSignal: (RoomSignal) -> Unit = {},
        ): RoomConnection {
            require(token.isNotBlank()) { "Sign in before joining the room." }
            val endpoint = roomWebSocketUrl(baseUrl)
            val request = Request.Builder()
                .url(endpoint)
                .header("Authorization", "Bearer $token")
                .build()
            val clientClosed = java.util.concurrent.atomic.AtomicBoolean(false)
            val outbound = RoomSocketOutbox()
            val webSocket = client.newWebSocket(
                request,
                object : WebSocketListener() {
                    override fun onOpen(webSocket: WebSocket, response: Response) {
                        if (!outbound.onOpen(webSocket)) {
                            reportError(onError, "Room connection closed before signaling could start.")
                            webSocket.close(1011, "Could not flush pending room messages")
                        }
                    }

                    override fun onMessage(webSocket: WebSocket, text: String) {
                        try {
                            val message = JSONObject(text)
                            when (message.optString("type")) {
                                "state" -> {
                                    val state = RoomState(
                                        revision = message.getLong("revision"),
                                        sourceKind = message.nullableString("source_kind"),
                                        source = message.nullableString("source"),
                                        title = message.nullableString("title"),
                                        playing = message.getBoolean("playing"),
                                        positionMs = message.getLong("position_ms"),
                                        serverTimeMs = message.getLong("server_time_ms"),
                                        updatedBy = message.nullableString("updated_by"),
                                        receivedElapsedMs = SystemClock.elapsedRealtime(),
                                    )
                                    mainHandler.post { onState(state) }
                                }
                                "signal" -> {
                                    val signal = message.toRoomSignal()
                                    mainHandler.post { onSignal(signal) }
                                }
                                "error" -> reportError(
                                    onError,
                                    message.optString("message").takeIf { it.isNotBlank() }
                                        ?: "The server rejected a room message.",
                                )
                            }
                        } catch (error: Exception) {
                            reportError(onError, "Received an invalid room message.")
                        }
                    }

                    override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                        outbound.close()
                        if (!clientClosed.get()) {
                            val detail = response?.let { " (HTTP ${it.code})" }.orEmpty()
                            reportError(onError, (t.message ?: "Room connection failed.") + detail)
                        }
                    }

                    override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                        outbound.close()
                        if (!clientClosed.get()) reportError(onError, "Room connection closed.")
                    }
                },
            )
            return RoomConnection(webSocket, clientClosed, outbound)
        }

        private fun roomWebSocketUrl(baseUrl: String): HttpUrl {
            val base = baseUrl.trim().trimEnd('/').toHttpUrlOrNull()
                ?: throw IllegalArgumentException("Enter a valid HTTPS server address.")
            require(
                base.isHttps && base.username.isEmpty() && base.password.isEmpty() &&
                    base.query == null && base.fragment == null
            ) { "The room requires a secure HTTPS server address." }
            return base.newBuilder()
                .addPathSegments("api/v1/room/ws")
                .build()
        }

        private fun reportError(onError: (String) -> Unit, message: String) {
            mainHandler.post { onError(message) }
        }

        private fun JSONObject.nullableString(name: String): String? {
            if (!has(name) || isNull(name)) return null
            return opt(name) as? String
                ?: throw IllegalArgumentException("Room field '$name' must be a string or null.")
        }

        private fun JSONObject.toRoomSignal(): RoomSignal {
            val from = getString("from").takeIf { it.isNotBlank() }
                ?: throw IllegalArgumentException("Room signal is missing its sender.")
            return when (val kind = getString("kind")) {
                "offer" -> RoomSignal.CallOffer(from, getString("sdp"))
                "answer" -> RoomSignal.CallAnswer(from, getString("sdp"))
                "candidate" -> RoomSignal.IceCandidate(
                    from,
                    getString("candidate"),
                    nullableString("sdp_mid"),
                    if (has("sdp_mline_index") && !isNull("sdp_mline_index")) getInt("sdp_mline_index") else null,
                )
                "end" -> RoomSignal.CallEnded(from)
                else -> throw IllegalArgumentException("Unsupported room signal kind.")
            }
        }
    }

    override fun close() {
        clientClosed.set(true)
        outbound.close()
        webSocket.close(NORMAL_CLOSE, "Room closed")
    }
}

/**
 * OkHttp creates a WebSocket before its opening handshake finishes. Calls can
 * generate ICE candidates during that interval, and WebSocket.send() returns
 * false until the socket is open. Hold a small, bounded FIFO and flush it from
 * onOpen so early room signals keep their order instead of failing the call.
 */
private class RoomSocketOutbox {
    private val lock = Any()
    private val pending = java.util.ArrayDeque<String>()
    private var isOpen = false
    private var isClosed = false
    private var pendingChars = 0

    fun send(webSocket: WebSocket, message: String): Boolean = synchronized(lock) {
        if (isClosed) return@synchronized false
        if (isOpen) return@synchronized webSocket.send(message)
        if (pending.size >= MAX_PENDING_MESSAGES || pendingChars + message.length > MAX_PENDING_CHARS) {
            return@synchronized false
        }
        pending.addLast(message)
        pendingChars += message.length
        true
    }

    fun onOpen(webSocket: WebSocket): Boolean = synchronized(lock) {
        if (isClosed) return@synchronized false
        isOpen = true
        while (pending.isNotEmpty()) {
            val message = pending.removeFirst()
            pendingChars -= message.length
            if (!webSocket.send(message)) {
                isOpen = false
                isClosed = true
                pending.clear()
                pendingChars = 0
                return@synchronized false
            }
        }
        true
    }

    fun close() = synchronized(lock) {
        isOpen = false
        isClosed = true
        pending.clear()
        pendingChars = 0
    }

    private companion object {
        const val MAX_PENDING_MESSAGES = 64
        const val MAX_PENDING_CHARS = 256 * 1024
    }
}

class MediaApi internal constructor(val baseUrl: String, private val token: String) {
    companion object {
        suspend fun login(base: String, username: String, password: String): LoginResult =
            withContext(Dispatchers.IO) {
                val normalized = base.trim().trimEnd('/')
                val uri = URI(normalized)
                require(uri.scheme == "https" && !uri.host.isNullOrBlank() && uri.userInfo == null) {
                    "Use the server’s HTTPS address."
                }
                val json = JSONObject().put("username", username).put("password", password)
                val response = request("POST", normalized + "/api/v1/auth/login", null, json.toString())
                LoginResult(response.getString("token"), response.getString("username"))
            }

        private fun request(method: String, address: String, token: String?, body: String? = null): JSONObject {
            val connection = (URL(address).openConnection() as HttpURLConnection).apply {
                requestMethod = method
                connectTimeout = 8_000
                readTimeout = 20_000
                setRequestProperty("Accept", "application/json")
                token?.let { setRequestProperty("Authorization", "Bearer " + it) }
                if (body != null) {
                    doOutput = true
                    setRequestProperty("Content-Type", "application/json; charset=utf-8")
                }
            }
            try {
                if (body != null) connection.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
                val status = connection.responseCode
                val stream = if (status in 200..299) connection.inputStream else connection.errorStream
                val text = stream?.bufferedReader()?.use { it.readText() }.orEmpty()
                if (status !in 200..299) error("Server returned HTTP " + status)
                return if (text.isBlank()) JSONObject() else JSONObject(text)
            } finally {
                connection.disconnect()
            }
        }
    }

    suspend fun list(path: String): List<MediaEntry> = withContext(Dispatchers.IO) {
        val encoded = URLEncoder.encode(path, Charsets.UTF_8.name())
        val response = request("GET", baseUrl + "/api/v1/media?path=" + encoded, token)
        val items = response.getJSONArray("items")
        buildList(items.length()) {
            for (index in 0 until items.length()) {
                val item = items.getJSONObject(index)
                add(MediaEntry(item.getString("name"), item.getString("path"), item.getString("kind"), item.optLong("size")))
            }
        }
    }

    suspend fun listSubtitles(mediaPath: String): List<SubtitleEntry> = withContext(Dispatchers.IO) {
        val encoded = URLEncoder.encode(mediaPath, Charsets.UTF_8.name())
        val response = request("GET", "$baseUrl/api/v1/subtitles?media=$encoded", token)
        val subtitles = response.getJSONArray("subtitles")
        buildList(subtitles.length()) {
            for (index in 0 until subtitles.length()) {
                val item = subtitles.getJSONObject(index)
                add(SubtitleEntry(item.getString("name"), item.getString("path"), item.getString("mime_type")))
            }
        }
    }

    suspend fun voiceIceServers(): List<VoiceIceServer> = withContext(Dispatchers.IO) {
        val response = request("GET", "$baseUrl/api/v1/voice/ice", token)
        val servers = response.getJSONArray("ice_servers")
        buildList(servers.length()) {
            for (index in 0 until servers.length()) {
                val server = servers.getJSONObject(index)
                val urls = server.getJSONArray("urls")
                add(
                    VoiceIceServer(
                        urls = buildList(urls.length()) { for (urlIndex in 0 until urls.length()) add(urls.getString(urlIndex)) },
                        username = if (server.has("username") && !server.isNull("username")) server.getString("username") else null,
                        credential = if (server.has("credential") && !server.isNull("credential")) server.getString("credential") else null,
                    ),
                )
            }
        }.also { require(it.isNotEmpty()) { "The server did not provide any voice network servers." } }
    }

    suspend fun startDownload(address: String): String = withContext(Dispatchers.IO) {
        val body = JSONObject().put("url", address).toString()
        request("POST", baseUrl + "/api/v1/downloads", token, body).getString("id")
    }

    suspend fun upload(
        input: InputStream,
        name: String,
        path: String,
        totalBytes: Long?,
        onProgress: (Long) -> Unit,
    ): UploadedMedia = withContext(Dispatchers.IO) {
        val encodedName = URLEncoder.encode(name, Charsets.UTF_8.name())
        val encodedPath = URLEncoder.encode(path, Charsets.UTF_8.name())
        val address = "$baseUrl/api/v1/uploads?path=$encodedPath&name=$encodedName"
        val connection = (URL(address).openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"
            connectTimeout = 15_000
            readTimeout = 120_000
            doOutput = true
            setRequestProperty("Authorization", "Bearer $token")
            setRequestProperty("Content-Type", "application/octet-stream")
            if (totalBytes != null && totalBytes >= 0L) {
                setFixedLengthStreamingMode(totalBytes)
            } else {
                setChunkedStreamingMode(64 * 1024)
            }
        }
        try {
            var sent = 0L
            var lastReported = 0L
            BufferedInputStream(input).use { source ->
                BufferedOutputStream(connection.outputStream).use { destination ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val count = source.read(buffer)
                        if (count < 0) break
                        destination.write(buffer, 0, count)
                        sent += count
                        if (sent - lastReported >= 256 * 1024 || (totalBytes != null && sent == totalBytes)) {
                            withContext(Dispatchers.Main) { onProgress(sent) }
                            lastReported = sent
                        }
                    }
                }
            }
            val status = connection.responseCode
            val responseStream = if (status in 200..299) connection.inputStream else connection.errorStream
            val response = responseStream?.bufferedReader()?.use { it.readText() }.orEmpty()
            if (status != 201) error("Upload failed (HTTP $status).")
            val json = JSONObject(response)
            UploadedMedia(json.getString("name"), json.getString("path"), json.getLong("size"))
        } finally {
            connection.disconnect()
        }
    }

    suspend fun createFolder(name: String, path: String) = withContext(Dispatchers.IO) {
        val body = JSONObject().put("name", name).put("path", path).toString()
        request("POST", baseUrl + "/api/v1/folders", token, body)
    }

    suspend fun liveConfig(): LiveConfig = withContext(Dispatchers.IO) {
        val response = request("GET", baseUrl + "/api/v1/live/config", token)
        LiveConfig(
            response.getString("stream_path"),
            response.getString("whip_url"),
            response.getString("rtmps_server_url"),
            response.getString("stream_key"),
        )
    }

    suspend fun liveIsActive(): Boolean = withContext(Dispatchers.IO) {
        request("GET", baseUrl + "/api/v1/live/status", token).optBoolean("active")
    }

    suspend fun resolveProvider(address: String): ResolvedMedia = withContext(Dispatchers.IO) {
        val body = JSONObject().put("url", address).toString()
        val response = request("POST", baseUrl + "/api/v1/media/resolve", token, body)
        val responseHeaders = response.optJSONObject("headers")
        val headers = buildMap {
            if (responseHeaders != null) {
                val names = responseHeaders.keys()
                while (names.hasNext()) {
                    val name = names.next()
                    val value = responseHeaders.optString(name).takeIf { it.isNotBlank() } ?: continue
                    put(name, value)
                }
            }
        }
        val audioUrl = if (response.has("audio_url") && !response.isNull("audio_url")) {
            response.getString("audio_url")
        } else null
        ResolvedMedia(response.getString("url"), audioUrl, response.getString("title"), headers)
    }

    suspend fun downloadStatus(id: String): DownloadStatus = withContext(Dispatchers.IO) {
        val response = request("GET", baseUrl + "/api/v1/downloads/" + id, token)
        DownloadStatus(
            response.getString("state"), response.optLong("bytes"),
            if (response.isNull("total")) null else response.optLong("total"),
            response.optString("saved_as").takeIf { it.isNotBlank() },
            response.optString("error").takeIf { it.isNotBlank() },
        )
    }

    suspend fun cancelDownload(id: String) = withContext(Dispatchers.IO) {
        request("POST", baseUrl + "/api/v1/downloads/" + id + "/cancel", token)
    }

    suspend fun logout() = withContext(Dispatchers.IO) {
        request("POST", baseUrl + "/api/v1/auth/logout", token)
    }

    fun streamUrl(path: String): String =
        baseUrl + "/api/v1/stream?path=" + URLEncoder.encode(path, Charsets.UTF_8)

    fun liveHlsUrl(path: String): String {
        val safePath = path.split('/').joinToString("/") {
            URLEncoder.encode(it, Charsets.UTF_8.name())
        }
        return "$baseUrl/api/v1/live/hls/$safePath/index.m3u8"
    }

    fun authHeader(): String = "Bearer " + token

    fun connectRoom(
        onState: (RoomState) -> Unit,
        onError: (String) -> Unit,
        onSignal: (RoomSignal) -> Unit = {},
    ): RoomConnection = RoomConnection.connect(baseUrl, token, onState, onError, onSignal)
}
