package site.ahura.mabaeiream

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
import android.os.SystemClock
import android.provider.OpenableColumns
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.enableEdgeToEdge
import androidx.activity.compose.setContent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.Image
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.runtime.*
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.path
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.media3.common.MediaItem as PlayerItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.C
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.source.MergingMediaSource
import androidx.media3.ui.PlayerView
import androidx.media3.ui.TrackSelectionDialogBuilder
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import kotlinx.coroutines.launch
import kotlinx.coroutines.delay
import kotlinx.coroutines.Job
import kotlinx.coroutines.sync.Mutex
import java.util.concurrent.atomic.AtomicLong

private val FolderIcon: ImageVector by lazy {
    ImageVector.Builder("Folder", 24.dp, 24.dp, 24f, 24f).apply {
        path(fill = SolidColor(Color.Black)) {
            moveTo(2f, 5f)
            lineTo(9f, 5f)
            lineTo(11f, 7f)
            lineTo(22f, 7f)
            lineTo(22f, 19f)
            lineTo(2f, 19f)
            close()
        }
    }.build()
}

private val DownloadIcon: ImageVector by lazy {
    ImageVector.Builder("Download", 24.dp, 24.dp, 24f, 24f).apply {
        path(fill = SolidColor(Color.Black)) {
            moveTo(11f, 3f)
            lineTo(11f, 13f)
            lineTo(7f, 9f)
            lineTo(5f, 11f)
            lineTo(12f, 18f)
            lineTo(19f, 11f)
            lineTo(17f, 9f)
            lineTo(13f, 13f)
            lineTo(13f, 3f)
            close()
            moveTo(5f, 20f)
            lineTo(19f, 20f)
            lineTo(19f, 22f)
            lineTo(5f, 22f)
            close()
        }
    }.build()
}

private data class AppPalette(
    val canvas: Color,
    val surface: Color,
    val surfaceSoft: Color,
    val surfaceRose: Color,
    val ink: Color,
    val muted: Color,
    val mutedSoft: Color,
    val line: Color,
    val primary: Color,
    val onPrimary: Color,
    val accent: Color,
    val accentInk: Color,
    val success: Color,
    val danger: Color,
    val dangerSoft: Color,
    val videoBackdrop: Color,
    val illustrationBench: Color,
    val illustrationHair: Color,
    val illustrationSkin: Color,
    val illustrationRose: Color,
    val illustrationGreen: Color,
    val illustrationLine: Color,
)

private val LightPalette = AppPalette(
    canvas = Color(0xFFF5F8F2), surface = Color(0xFFFFFEFB), surfaceSoft = Color(0xFFEDF3EC),
    surfaceRose = Color(0xFFF6E5E9),
    ink = Color(0xFF23362D), muted = Color(0xFF58695F), mutedSoft = Color(0xFF626F65),
    line = Color(0xFFDCE5DA), primary = Color(0xFF286B50), onPrimary = Color.White,
    accent = Color(0xFF924359), accentInk = Color(0xFF873C53), success = Color(0xFF286B50),
    danger = Color(0xFF983D4C), dangerSoft = Color(0xFFF8E6E8), videoBackdrop = Color.Black,
    illustrationBench = Color(0xFF9D735F), illustrationHair = Color(0xFF62473F),
    illustrationSkin = Color(0xFFFFD7C2), illustrationRose = Color(0xFFE87988),
    illustrationGreen = Color(0xFF4F805C), illustrationLine = Color(0xFF6A5148),
)

private val DarkPalette = AppPalette(
    canvas = Color(0xFF161D17), surface = Color(0xFF202A23), surfaceSoft = Color(0xFF29372D),
    surfaceRose = Color(0xFF3C2931),
    ink = Color(0xFFF1F3EF), muted = Color(0xFFBBC7BD), mutedSoft = Color(0xFF98A69B),
    line = Color(0xFF39483D), primary = Color(0xFF8CCBA5), onPrimary = Color(0xFF183B27),
    accent = Color(0xFFF1A7B8), accentInk = Color(0xFFF3BECA), success = Color(0xFF8CCBA5),
    danger = Color(0xFFF1A5B0), dangerSoft = Color(0xFF42282E), videoBackdrop = Color.Black,
    illustrationBench = Color(0xFF9D735F), illustrationHair = Color(0xFF62473F),
    illustrationSkin = Color(0xFFFFD7C2), illustrationRose = Color(0xFFE87988),
    illustrationGreen = Color(0xFF4F805C), illustrationLine = Color(0xFF6A5148),
)

private val LocalAppPalette = staticCompositionLocalOf { LightPalette }

private object Colors {
    val canvas: Color @Composable get() = LocalAppPalette.current.canvas
    val surface: Color @Composable get() = LocalAppPalette.current.surface
    val surfaceSoft: Color @Composable get() = LocalAppPalette.current.surfaceSoft
    val surfaceRose: Color @Composable get() = LocalAppPalette.current.surfaceRose
    val ink: Color @Composable get() = LocalAppPalette.current.ink
    val muted: Color @Composable get() = LocalAppPalette.current.muted
    val mutedSoft: Color @Composable get() = LocalAppPalette.current.mutedSoft
    val line: Color @Composable get() = LocalAppPalette.current.line
    val primary: Color @Composable get() = LocalAppPalette.current.primary
    val onPrimary: Color @Composable get() = LocalAppPalette.current.onPrimary
    val accent: Color @Composable get() = LocalAppPalette.current.accent
    val accentInk: Color @Composable get() = LocalAppPalette.current.accentInk
    val success: Color @Composable get() = LocalAppPalette.current.success
    val danger: Color @Composable get() = LocalAppPalette.current.danger
    val dangerSoft: Color @Composable get() = LocalAppPalette.current.dangerSoft
    val videoBackdrop: Color @Composable get() = LocalAppPalette.current.videoBackdrop
    val illustrationBench: Color @Composable get() = LocalAppPalette.current.illustrationBench
    val illustrationHair: Color @Composable get() = LocalAppPalette.current.illustrationHair
    val illustrationSkin: Color @Composable get() = LocalAppPalette.current.illustrationSkin
    val illustrationRose: Color @Composable get() = LocalAppPalette.current.illustrationRose
    val illustrationGreen: Color @Composable get() = LocalAppPalette.current.illustrationGreen
    val illustrationLine: Color @Composable get() = LocalAppPalette.current.illustrationLine
}

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            val useDarkPalette = isSystemInDarkTheme()
            val palette = if (useDarkPalette) DarkPalette else LightPalette
            CompositionLocalProvider(LocalAppPalette provides palette) {
                val colorScheme = if (useDarkPalette) {
                    darkColorScheme(
                        primary = palette.primary, onPrimary = palette.onPrimary,
                        primaryContainer = palette.surfaceSoft, onPrimaryContainer = palette.ink,
                        secondary = palette.accent, onSecondary = palette.onPrimary,
                        secondaryContainer = palette.surfaceRose, onSecondaryContainer = palette.accentInk,
                        error = palette.danger, onError = palette.onPrimary,
                        background = palette.canvas, surface = palette.surface,
                        surfaceVariant = palette.surfaceSoft, onSurface = palette.ink,
                        onBackground = palette.ink, onSurfaceVariant = palette.muted,
                        outline = palette.line, outlineVariant = palette.line, surfaceTint = palette.primary,
                    )
                } else {
                    lightColorScheme(
                        primary = palette.primary, onPrimary = palette.onPrimary,
                        primaryContainer = palette.surfaceSoft, onPrimaryContainer = palette.ink,
                        secondary = palette.accent, onSecondary = Color.White,
                        secondaryContainer = palette.surfaceRose, onSecondaryContainer = palette.accentInk,
                        error = palette.danger, onError = palette.onPrimary,
                        background = palette.canvas, surface = palette.surface,
                        surfaceVariant = palette.surfaceSoft, onSurface = palette.ink,
                        onBackground = palette.ink, onSurfaceVariant = palette.muted,
                        outline = palette.line, outlineVariant = palette.line, surfaceTint = palette.primary,
                    )
                }
                MaterialTheme(colorScheme = colorScheme) {
                    Surface(
                        modifier = Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing),
                        color = Colors.canvas,
                    ) {
                        MaBaeireamApp()
                    }
                }
            }
        }
    }
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> if (baseContext === this) null else baseContext.findActivity()
    else -> null
}

private sealed interface LoadStatus {
    data object Idle : LoadStatus
    data object Loading : LoadStatus
    data object Ready : LoadStatus
    data class Error(val message: String) : LoadStatus
}

private data class PlaybackSubtitle(val url: String, val mimeType: String, val label: String)

private data class PlaybackSource(
    val url: String,
    val title: String,
    val authHeader: String?,
    val positionMs: Long = 0L,
    val playing: Boolean = false,
    val requestHeaders: Map<String, String> = emptyMap(),
    val audioUrl: String? = null,
    val isLive: Boolean = false,
    val subtitles: List<PlaybackSubtitle> = emptyList(),
)

@Composable
private fun MaBaeireamApp() {
    var api by remember { mutableStateOf<MediaApi?>(null) }
    var signedInAs by remember { mutableStateOf("") }
    var items by remember { mutableStateOf<List<MediaEntry>>(emptyList()) }
    var folder by remember { mutableStateOf("") }
    var currentMedia by remember { mutableStateOf<PlaybackSource?>(null) }
    var status by remember { mutableStateOf<LoadStatus>(LoadStatus.Idle) }
    var link by remember { mutableStateOf("") }
    var transferMessage by remember { mutableStateOf("") }
    var activeDownloadId by remember { mutableStateOf<String?>(null) }
    var selectedTab by remember { mutableStateOf(0) }
    var activeUploadName by remember { mutableStateOf("") }
    var uploadBytes by remember { mutableStateOf(0L) }
    var uploadTotal by remember { mutableStateOf<Long?>(null) }
    var isUploading by remember { mutableStateOf(false) }
    var liveConfig by remember { mutableStateOf<LiveConfig?>(null) }
    var liveConfigError by remember { mutableStateOf("") }
    var liveActive by remember { mutableStateOf(false) }
    var roomConnection by remember { mutableStateOf<RoomConnection?>(null) }
    var roomState by remember { mutableStateOf<RoomState?>(null) }
    val localControlPendingUntil = remember { AtomicLong(0L) }
    var roomError by remember { mutableStateOf("") }
    var voiceState by remember { mutableStateOf<VoiceCallState>(VoiceCallState.Idle) }
    var speakerphoneOn by remember { mutableStateOf(true) }
    var voiceController by remember { mutableStateOf<WebRtcCallController?>(null) }
    var voiceIceServers by remember { mutableStateOf<List<VoiceIceServer>?>(null) }
    var pendingVoiceAction by remember { mutableStateOf(false) }
    var pendingVoiceIceServers by remember { mutableStateOf<List<VoiceIceServer>?>(null) }
    var microphoneMessage by remember { mutableStateOf("") }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val voiceIceMutex = remember { Mutex() }

    if (api == null) {
        LoginScreen(status = status, onLogin = { server, username, password ->
            status = LoadStatus.Loading
            scope.launch {
                runCatching { MediaApi.login(server, username, password) }
                    .onSuccess { session ->
                        api = MediaApi(server.trim().trimEnd('/'), session.token)
                        signedInAs = session.username
                        status = LoadStatus.Loading
                        runCatching { api!!.list("") }
                            .onSuccess { items = it; status = LoadStatus.Ready }
                            .onFailure { status = LoadStatus.Error(it.message ?: "Could not load the library.") }
                    }
                    .onFailure { status = LoadStatus.Error(it.message ?: "Could not sign in.") }
            }
        })
        return
    }

    val session = api!!
    val uploadPicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenMultipleDocuments(),
    ) { selectedFiles ->
        if (selectedFiles.isNotEmpty()) scope.launch {
            isUploading = true
            var uploaded = 0
            for (uri in selectedFiles) {
                val resolver = context.contentResolver
                val metadata = runCatching {
                    resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)
                        ?.use { cursor ->
                            if (!cursor.moveToFirst()) null else {
                                val nameColumn = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                                val sizeColumn = cursor.getColumnIndex(OpenableColumns.SIZE)
                                val name = if (nameColumn >= 0) cursor.getString(nameColumn) else null
                                val size = if (sizeColumn >= 0 && !cursor.isNull(sizeColumn)) cursor.getLong(sizeColumn) else null
                                name to size
                            }
                        }
                }.getOrNull()?.takeIf { !it.first.isNullOrBlank() }
                val name = metadata?.first ?: uri.lastPathSegment?.substringAfterLast('/') ?: "upload-${System.currentTimeMillis()}"
                val size = metadata?.second
                activeUploadName = name
                uploadBytes = 0L
                uploadTotal = size
                transferMessage = "Uploading $name…"
                runCatching {
                    val stream = resolver.openInputStream(uri) ?: error("Could not open $name.")
                    session.upload(stream, name, folder, size) { sent -> uploadBytes = sent }
                }.onSuccess {
                    uploaded++
                    transferMessage = "Uploaded ${it.name} · ${formatSize(it.size)}"
                }.onFailure {
                    transferMessage = it.message ?: "Could not upload $name."
                }
            }
            runCatching { session.list(folder) }.onSuccess { items = it; status = LoadStatus.Ready }
            isUploading = false
            activeUploadName = ""
            uploadTotal = null
            if (uploaded > 0) selectedTab = 1
        }
    }
    suspend fun requireVoiceIceServers(): List<VoiceIceServer> {
        voiceIceMutex.lock()
        try {
            voiceIceServers?.let { return it }
            return session.voiceIceServers().also { voiceIceServers = it }
        } finally {
            voiceIceMutex.unlock()
        }
    }

    fun getVoiceController(iceServers: List<VoiceIceServer>): WebRtcCallController? {
        voiceController?.let { return it }
        return try {
            WebRtcCallController(
                context = context.applicationContext,
                voiceIceServers = iceServers,
                sendOffer = { roomConnection?.sendCallOffer(it) == true },
                sendAnswer = { roomConnection?.sendCallAnswer(it) == true },
                sendCandidate = { candidate, mid, index ->
                    roomConnection?.sendIceCandidate(candidate, mid, index) == true
                },
                sendEnd = { roomConnection?.sendCallEnd() == true },
                onState = { voiceState = it },
                onSpeakerphoneChanged = { speakerphoneOn = it },
            ).also { voiceController = it }
        } catch (error: Exception) {
            voiceState = VoiceCallState.Failed(error.message ?: "Voice chat could not start on this device.")
            null
        } catch (error: LinkageError) {
            voiceState = VoiceCallState.Failed("The voice calling component is unavailable in this app build.")
            null
        }
    }

    val microphonePermission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        val acceptIncoming = pendingVoiceAction
        val iceServers = pendingVoiceIceServers
        pendingVoiceAction = false
        pendingVoiceIceServers = null
        if (granted) {
            microphoneMessage = ""
            iceServers?.let { getVoiceController(it) }?.let { controller ->
                if (acceptIncoming) controller.acceptIncoming() else controller.startOutgoing()
            }
        } else {
            microphoneMessage = "Microphone access is needed for voice chat. You can still watch together without it."
        }
    }

    fun requestVoiceAction() {
        val accept = voiceState is VoiceCallState.Incoming
        scope.launch {
            val iceServers = runCatching { requireVoiceIceServers() }.getOrElse {
                voiceState = VoiceCallState.Failed(it.message ?: "Could not load voice network settings.")
                return@launch
            }
            if (context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
                getVoiceController(iceServers)?.let { controller ->
                    if (accept) controller.acceptIncoming() else controller.startOutgoing()
                }
            } else {
                pendingVoiceAction = accept
                pendingVoiceIceServers = iceServers
                microphonePermission.launch(Manifest.permission.RECORD_AUDIO)
            }
        }
    }

    LaunchedEffect(session) {
        runCatching { requireVoiceIceServers() }
    }

    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(session, lifecycleOwner) {
        var backgroundHangup: Job? = null
        val lifecycleObserver = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_STOP -> {
                    backgroundHangup?.cancel()
                    backgroundHangup = scope.launch {
                        delay(3_000L)
                        if (!lifecycleOwner.lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) {
                            voiceController?.endAfterBackgroundTimeout()
                        }
                    }
                }
                Lifecycle.Event.ON_START -> {
                    backgroundHangup?.cancel()
                    backgroundHangup = null
                }
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(lifecycleObserver)
        val connection = runCatching {
            session.connectRoom(
                onState = { state ->
                    val previousRevision = roomState?.revision
                    roomState = state
                    if (previousRevision == null || previousRevision != state.revision) {
                        localControlPendingUntil.set(0L)
                    }
                },
                onError = { roomError = it },
                onSignal = { signal ->
                    scope.launch {
                        runCatching {
                            getVoiceController(requireVoiceIceServers())?.handleSignal(signal)
                        }.onFailure {
                            voiceState = VoiceCallState.Failed(it.message ?: "Could not load voice network settings.")
                        }
                    }
                },
            )
        }
        connection.onSuccess {
            roomConnection = it
            roomError = ""
        }.onFailure {
            roomError = it.message ?: "Could not connect to the shared room."
        }
        onDispose {
            backgroundHangup?.cancel()
            lifecycleOwner.lifecycle.removeObserver(lifecycleObserver)
            voiceController?.close()
            voiceController = null
            roomConnection?.close()
            roomConnection = null
        }
    }
    LaunchedEffect(session, roomState?.sourceKind, roomState?.source, roomState?.title) {
        val state = roomState ?: return@LaunchedEffect
        val source = state.source ?: return@LaunchedEffect
        val title = state.title ?: return@LaunchedEffect
        val kind = state.sourceKind ?: return@LaunchedEffect
        currentMedia = null
        roomError = ""
        runCatching {
            if (kind == "library") {
                val subtitles = runCatching { session.listSubtitles(source) }
                    .getOrDefault(emptyList())
                    .map { PlaybackSubtitle(session.streamUrl(it.path), it.mimeType, it.name) }
                PlaybackSource(
                    session.streamUrl(source), title, session.authHeader(),
                    state.positionMs, state.playing,
                    subtitles = subtitles,
                )
            } else if (kind == "live") {
                PlaybackSource(
                    session.liveHlsUrl(source), title, session.authHeader(),
                    state.positionMs, state.playing, isLive = true,
                )
            } else if (kind == "https" && requiresProviderResolution(source)) {
                val resolved = session.resolveProvider(source)
                PlaybackSource(
                    resolved.url, resolved.title, null,
                    state.positionMs, state.playing, resolved.headers, resolved.audioUrl,
                )
            } else {
                PlaybackSource(source, title, null, state.positionMs, state.playing)
            }
        }.onSuccess { currentMedia = it }
            .onFailure { roomError = it.message ?: "Could not prepare the shared video." }
    }
    LaunchedEffect(session) {
        runCatching { session.liveConfig() }
            .onSuccess { liveConfig = it; liveConfigError = "" }
            .onFailure { liveConfig = null; liveConfigError = it.message ?: "Desktop streaming is not available on this server." }
    }
    LaunchedEffect(session, selectedTab) {
        if (selectedTab == 3) {
            while (true) {
                runCatching { session.liveIsActive() }.onSuccess { liveActive = it }
                delay(2_500L)
            }
        }
    }
    val video = currentMedia
    BackHandler(enabled = video != null) { currentMedia = null }
    Surface(Modifier.fillMaxSize(), color = Colors.canvas) {
        Box(Modifier.fillMaxSize()) {
            if (video != null) {
                PlayerScreen(
                    video = video,
                    roomState = roomState,
                    localControlPendingUntil = localControlPendingUntil,
                    onPlayback = { playing, position ->
                        localControlPendingUntil.set(SystemClock.elapsedRealtime() + 1_500L)
                        if (playing) roomConnection?.play(position) else roomConnection?.pause(position)
                    },
                    onSeek = { position ->
                        localControlPendingUntil.set(SystemClock.elapsedRealtime() + 1_500L)
                        roomConnection?.seek(position)
                    },
                    onBack = { currentMedia = null },
                )
            } else {
                RedesignedLibraryScreen(
                user = signedInAs, folder = folder, items = items, status = status, link = link,
                roomState = roomState, roomError = roomError,
                transferMessage = transferMessage,
                isDownloading = activeDownloadId != null,
                selectedTab = selectedTab,
                isUploading = isUploading,
                uploadName = activeUploadName,
                uploadBytes = uploadBytes,
                uploadTotal = uploadTotal,
                voiceState = voiceState,
                microphoneMessage = microphoneMessage,
                speakerphoneOn = speakerphoneOn,
                liveConfig = liveConfig,
                liveConfigError = liveConfigError,
                liveActive = liveActive,
                roomConnection = roomConnection,
                onTabSelected = { selectedTab = it },
                onUpload = { uploadPicker.launch(arrayOf("*/*")) },
                onCreateFolder = { name ->
                    scope.launch {
                        runCatching { session.createFolder(name, folder) }
                            .onSuccess {
                                transferMessage = "Folder created: $name"
                                runCatching { session.list(folder) }.onSuccess { items = it; status = LoadStatus.Ready }
                            }
                            .onFailure { transferMessage = it.message ?: "Could not create the folder." }
                    }
                },
                onCallOrAccept = ::requestVoiceAction,
                onDecline = { voiceController?.declineIncoming() },
                onHangUp = { voiceController?.hangUp() },
                onToggleSpeakerphone = { voiceController?.toggleSpeakerphone() },
                onDismissCallError = { voiceController?.dismissError(); microphoneMessage = "" },
                onLinkChange = { link = it },
                onPlayLink = {
                    val value = link.trim()
                    val uri = runCatching { Uri.parse(value) }.getOrNull()
                    val host = uri?.host?.lowercase().orEmpty()
                    when {
                        uri == null || uri.scheme != "https" || uri.host.isNullOrBlank() || uri.userInfo != null ->
                            status = LoadStatus.Error("Paste an HTTPS media or provider link.")
                        else -> {
                            roomConnection?.let { room ->
                                val title = when {
                                    host == "youtube.com" || host.endsWith(".youtube.com") || host == "youtu.be" -> "YouTube video"
                                    host == "vimeo.com" || host.endsWith(".vimeo.com") -> "Vimeo video"
                                    else -> uri.lastPathSegment?.takeIf { it.isNotBlank() } ?: "Shared link"
                                }
                                room.select("https", value, title)
                            }
                        }
                    }
                },
                onDownloadLink = {
                    val value = link.trim()
                    if (!value.startsWith("https://", ignoreCase = true) && !value.startsWith("http://", ignoreCase = true)) {
                        transferMessage = "Use a direct HTTP or HTTPS file link."
                    } else {
                        transferMessage = "Starting download…"
                        scope.launch {
                            runCatching {
                                val id = session.startDownload(value)
                                activeDownloadId = id
                                var result = session.downloadStatus(id)
                                while (result.state == "queued" || result.state == "downloading") {
                                    transferMessage = if (result.total != null && result.total > 0L) {
                                        "Downloading · " + (result.bytes * 100 / result.total) + "%"
                                    } else {
                                        "Downloading · " + formatSize(result.bytes)
                                    }
                                    delay(750)
                                    result = session.downloadStatus(id)
                                }
                                activeDownloadId = null
                                result
                            }.onSuccess { result ->
                                activeDownloadId = null
                                if (result.state == "complete") {
                                    transferMessage = "Saved to " + (result.savedAs ?: "the library")
                                    runCatching { session.list(folder) }.onSuccess { items = it }
                                } else if (result.state == "cancelled") {
                                    transferMessage = "Download cancelled."
                                } else {
                                    transferMessage = result.error ?: "The download failed."
                                }
                            }.onFailure {
                                activeDownloadId = null
                                transferMessage = it.message ?: "Could not start the download."
                            }
                        }
                    }
                },
                onCancelDownload = {
                    activeDownloadId?.let { downloadId ->
                        transferMessage = "Cancelling download…"
                        scope.launch {
                            runCatching { session.cancelDownload(downloadId) }
                            transferMessage = "Download cancelled."
                            activeDownloadId = null
                        }
                    }
                },
                onUp = {
                    folder = folder.substringBeforeLast("/", "")
                    items = emptyList()
                    status = LoadStatus.Loading
                    scope.launch {
                        runCatching { session.list(folder) }
                            .onSuccess { items = it; status = LoadStatus.Ready }
                            .onFailure { status = LoadStatus.Error(it.message ?: "Could not open parent folder.") }
                    }
                },
                onRefresh = {
                    status = LoadStatus.Loading
                    scope.launch {
                        runCatching { session.list(folder) }
                            .onSuccess { items = it; status = LoadStatus.Ready }
                            .onFailure { status = LoadStatus.Error(it.message ?: "Could not refresh the library.") }
                    }
                },
                onOpen = { entry ->
                    if (entry.kind == "folder") {
                        folder = entry.path
                        items = emptyList()
                        status = LoadStatus.Loading
                        scope.launch {
                            runCatching { session.list(entry.path) }
                                .onSuccess { items = it; status = LoadStatus.Ready }
                                .onFailure { status = LoadStatus.Error(it.message ?: "Could not open folder.") }
                        }
                    } else if (entry.kind == "video") {
                        roomConnection?.let { room ->
                            room.select("library", entry.path, entry.name)
                        }
                    }
                },
                    onSignOut = {
                        voiceController?.hangUp()
                        activeDownloadId = null
                        scope.launch { runCatching { session.logout() } }
                        api = null; signedInAs = ""; items = emptyList(); status = LoadStatus.Idle
                    },
                )
            }
        }
    }
}

@Composable
private fun VoiceCallControls(
    state: VoiceCallState,
    permissionMessage: String,
    speakerphoneOn: Boolean,
    onCallOrAccept: () -> Unit,
    onDecline: () -> Unit,
    onHangUp: () -> Unit,
    onToggleSpeakerphone: () -> Unit,
    onDismissError: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(containerColor = Colors.surfaceRose),
        shape = RoundedCornerShape(26.dp),
    ) {
        Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("OUR LITTLE VOICE ROOM", color = Colors.accentInk, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.25.sp)
                    Text("A call makes it cozier.", color = Colors.ink, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(top = 3.dp))
                }
                CoupleAvatars()
            }
            if (permissionMessage.isNotBlank()) Text(permissionMessage, color = Colors.ink, fontSize = 11.sp)
            when (state) {
                VoiceCallState.Idle -> Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("Start a private voice chat while you watch.", color = Colors.muted, fontSize = 11.sp, modifier = Modifier.weight(1f).padding(end = 10.dp))
                    Box(
                        Modifier.size(76.dp).background(Colors.surfaceRose, CircleShape).clickable(role = Role.Button, onClick = onCallOrAccept)
                            .semantics { contentDescription = "Start voice chat" },
                        contentAlignment = Alignment.Center,
                    ) {
                        Box(Modifier.size(58.dp).background(Colors.primary, CircleShape), contentAlignment = Alignment.Center) {
                            Icon(Icons.Default.Call, contentDescription = null, tint = Colors.onPrimary, modifier = Modifier.size(23.dp))
                        }
                    }
                }
                is VoiceCallState.Incoming -> {
                    Text("${state.from} is calling", color = Colors.ink, fontWeight = FontWeight.SemiBold, fontSize = 14.sp)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Button(onClick = onCallOrAccept, colors = ButtonDefaults.buttonColors(containerColor = Colors.primary, contentColor = Colors.onPrimary), shape = RoundedCornerShape(13.dp)) { Text("Answer") }
                        OutlinedButton(onClick = onDecline, border = androidx.compose.foundation.BorderStroke(1.dp, Colors.line), shape = RoundedCornerShape(13.dp)) { Text("Decline", color = Colors.ink) }
                    }
                }
                VoiceCallState.Calling -> CallStatus("Calling…", speakerphoneOn, onHangUp, onToggleSpeakerphone)
                VoiceCallState.Connecting -> CallStatus("Connecting voice…", speakerphoneOn, onHangUp, onToggleSpeakerphone)
                VoiceCallState.Connected -> CallStatus("You’re together · Voice connected", speakerphoneOn, onHangUp, onToggleSpeakerphone)
                is VoiceCallState.Failed -> {
                    Text(state.message, color = Colors.danger, fontSize = 12.sp)
                    TextButton(onClick = onDismissError, contentPadding = PaddingValues(0.dp)) { Text("Dismiss", color = Colors.ink) }
                }
            }
        }
    }
}

@Composable
private fun CallStatus(label: String, speakerphoneOn: Boolean, onHangUp: () -> Unit, onToggleSpeakerphone: () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(label, color = Colors.ink, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
            TextButton(onClick = onHangUp) { Text("End", color = Colors.danger) }
        }
        TextButton(onClick = onToggleSpeakerphone, contentPadding = PaddingValues(horizontal = 0.dp, vertical = 2.dp)) {
            Text(if (speakerphoneOn) "Speakerphone on · switch audio output" else "Speakerphone off · switch audio output", color = Colors.muted, fontSize = 12.sp)
        }
    }
}

@Composable
private fun LoginScreen(status: LoadStatus, onLogin: (String, String, String) -> Unit) {
    var server by remember { mutableStateOf("https://ahura.site/mabaeiream") }
    var username by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }

    Column(Modifier.fillMaxSize().background(Colors.canvas).verticalScroll(rememberScrollState()).padding(horizontal = 21.dp, vertical = 18.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Image(painterResource(R.drawable.mabaeiream_brand), contentDescription = "MaBaeiream icon", modifier = Modifier.size(51.dp))
            Column(Modifier.padding(start = 11.dp)) {
                Text("MaBaeiream", color = Colors.ink, fontSize = 17.sp, fontWeight = FontWeight.Bold)
                Text("a little place for us", color = Colors.muted, fontSize = 11.sp)
            }
        }
        Spacer(Modifier.height(19.dp))
        Card(colors = CardDefaults.cardColors(containerColor = Colors.surfaceRose), shape = RoundedCornerShape(25.dp)) {
            Column(Modifier.fillMaxWidth().padding(19.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text("A SHARED SPACE FOR TWO", color = Colors.accentInk, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.35.sp)
                Text("Your library,\ntogether.", color = Colors.ink, fontSize = 31.sp, lineHeight = 34.sp, fontFamily = FontFamily.Serif)
                Text("Films, little moments, and time with each other.", color = Colors.muted, fontSize = 12.sp)
                CoupleIllustration(Modifier.fillMaxWidth().height(126.dp).padding(top = 2.dp))
            }
        }
        Spacer(Modifier.height(19.dp))
        Text("Come on in", color = Colors.ink, fontSize = 21.sp, fontWeight = FontWeight.SemiBold)
        Text("Sign in to your private server.", color = Colors.muted, fontSize = 12.sp, modifier = Modifier.padding(top = 3.dp, bottom = 11.dp))
        FormField("Secure server address", server, { server = it }, "https://media.example.com")
        Spacer(Modifier.height(10.dp))
        FormField("Username", username, { username = it }, "Your account")
        Spacer(Modifier.height(10.dp))
        OutlinedTextField(
            value = password, onValueChange = { password = it }, label = { Text("Password") },
            modifier = Modifier.fillMaxWidth(), singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            colors = fieldColors(), shape = RoundedCornerShape(15.dp),
        )
        Spacer(Modifier.height(14.dp))
        when (status) {
            LoadStatus.Loading -> LinearProgressIndicator(Modifier.fillMaxWidth(), color = Colors.success)
            is LoadStatus.Error -> Text(status.message, color = Colors.danger, fontSize = 13.sp)
            else -> Unit
        }
        Spacer(Modifier.height(12.dp))
        Button(
            onClick = { onLogin(server, username, password) },
            enabled = status != LoadStatus.Loading && server.isNotBlank() && username.isNotBlank() && password.isNotBlank(),
            modifier = Modifier.fillMaxWidth().height(52.dp),
            colors = ButtonDefaults.buttonColors(containerColor = Colors.primary, contentColor = Colors.onPrimary),
            shape = RoundedCornerShape(15.dp),
        ) { Text("Enter our space", fontWeight = FontWeight.Bold) }
        Text("HTTPS keeps your connection protected. Your password is never saved on this device.", color = Colors.muted, fontSize = 11.sp, modifier = Modifier.padding(top = 11.dp, bottom = 12.dp))
    }
}

@Composable
private fun CopyStreamValue(label: String, value: String, context: Context) {
    Row(
        Modifier.fillMaxWidth().padding(top = 7.dp).background(Colors.surface.copy(alpha = 0.72f), RoundedCornerShape(12.dp))
            .padding(start = 11.dp, end = 4.dp, top = 7.dp, bottom = 7.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(label, color = Colors.muted, fontSize = 10.sp)
            Text(value, color = Colors.ink, fontSize = 11.sp, maxLines = 2)
        }
        TextButton(
            onClick = {
                val clipboard = context.getSystemService(android.content.ClipboardManager::class.java)
                clipboard?.setPrimaryClip(android.content.ClipData.newPlainText(label, value))
            },
        ) { Text("Copy", color = Colors.primary, fontSize = 11.sp) }
    }
}

@Composable
private fun MediaRow(entry: MediaEntry, onClick: () -> Unit) {
    val tint = when (entry.kind) { "folder" -> Colors.surfaceSoft; "video" -> Colors.surfaceRose; else -> Colors.surfaceSoft }
    Row(
        Modifier.fillMaxWidth().padding(vertical = 4.dp).background(Colors.surface, RoundedCornerShape(19.dp))
            .clickable(role = Role.Button, onClick = onClick).padding(horizontal = 13.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(47.dp).background(tint, RoundedCornerShape(15.dp)), contentAlignment = Alignment.Center) {
            Text(
                if (entry.kind == "folder") "▰" else if (entry.kind == "video") "▶" else "▤",
                color = if (entry.kind == "video") Colors.primary else Colors.primary, fontSize = 19.sp, fontWeight = FontWeight.Bold,
            )
        }
        Column(Modifier.weight(1f).padding(start = 12.dp)) {
            Text(entry.name, color = Colors.ink, fontWeight = FontWeight.Medium, maxLines = 1)
            Text(if (entry.kind == "folder") "Folder" else formatSize(entry.size), color = Colors.muted, fontSize = 12.sp)
        }
        Text(if (entry.kind == "video") "Watch  ›" else "›", color = Colors.primary, fontSize = 13.sp)
    }
}

@Composable
private fun RedesignedLibraryScreen(
    user: String, folder: String, items: List<MediaEntry>, status: LoadStatus, link: String,
    roomState: RoomState?, roomError: String, transferMessage: String, isDownloading: Boolean,
    selectedTab: Int, isUploading: Boolean, uploadName: String, uploadBytes: Long, uploadTotal: Long?,
    voiceState: VoiceCallState, microphoneMessage: String, speakerphoneOn: Boolean,
    liveConfig: LiveConfig?, liveConfigError: String, liveActive: Boolean, roomConnection: RoomConnection?,
    onTabSelected: (Int) -> Unit, onUpload: () -> Unit, onCreateFolder: (String) -> Unit,
    onCallOrAccept: () -> Unit, onDecline: () -> Unit, onHangUp: () -> Unit,
    onToggleSpeakerphone: () -> Unit, onDismissCallError: () -> Unit,
    onLinkChange: (String) -> Unit, onPlayLink: () -> Unit,
    onDownloadLink: () -> Unit, onCancelDownload: () -> Unit, onUp: () -> Unit, onRefresh: () -> Unit,
    onOpen: (MediaEntry) -> Unit, onSignOut: () -> Unit,
) {
    var query by remember { mutableStateOf("") }
    var showFolderDialog by remember { mutableStateOf(false) }
    var newFolderName by remember { mutableStateOf("") }
    val visible = items.filter { it.name.contains(query, ignoreCase = true) }
    val tabs = listOf("Home", "Files", "Downloads", "Watch")
    val tabIcons = listOf(Icons.Filled.Home, FolderIcon, DownloadIcon, Icons.Filled.PlayArrow)

    Scaffold(
        containerColor = Colors.canvas,
        bottomBar = {
            Row(
                Modifier.fillMaxWidth().background(Colors.surface).padding(horizontal = 10.dp, vertical = 7.dp),
                horizontalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                tabs.forEachIndexed { index, title ->
                    Column(
                        Modifier.weight(1f).height(56.dp)
                            .background(if (selectedTab == index) Colors.surfaceSoft else Color.Transparent, RoundedCornerShape(17.dp))
                            .clickable(role = Role.Tab) { onTabSelected(index) }
                            .semantics {
                                contentDescription = listOf("Your room and voice chat", "Shared files", "Downloads", "Watch together")[index]
                                selected = selectedTab == index
                            },
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center,
                    ) {
                        Icon(tabIcons[index], contentDescription = null, tint = if (selectedTab == index) Colors.primary else Colors.muted, modifier = Modifier.size(20.dp))
                        Text(title, color = if (selectedTab == index) Colors.ink else Colors.muted, fontSize = 10.sp, fontWeight = if (selectedTab == index) FontWeight.SemiBold else FontWeight.Normal)
                    }
                }
            }
        },
    ) { inset ->
        Column(Modifier.fillMaxSize().padding(inset)) {
            Row(
                Modifier.fillMaxWidth().padding(start = 18.dp, end = 12.dp, top = 7.dp, bottom = 5.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Image(painterResource(R.drawable.mabaeiream_brand), "MaBaeiream", Modifier.size(43.dp))
                Column(Modifier.weight(1f).padding(start = 10.dp)) {
                    Text("MaBaeiream", color = Colors.ink, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                    Text("your little corner together", color = Colors.muted, fontSize = 10.sp)
                }
                TextButton(onClick = onSignOut, contentPadding = PaddingValues(horizontal = 9.dp, vertical = 6.dp)) {
                    Text("Sign out", color = Colors.muted, fontSize = 12.sp)
                }
            }
            when (selectedTab) {
                0 -> CoupleHomePage(user, roomState, roomError, items, voiceState, microphoneMessage, speakerphoneOn, onTabSelected, onOpen, onCallOrAccept, onDecline, onHangUp, onToggleSpeakerphone, onDismissCallError)
                1 -> KeepsakesPage(folder, visible, items.size, status, transferMessage, isUploading, uploadName, uploadBytes, uploadTotal, query, { query = it }, onUpload, { showFolderDialog = true }, onUp, onRefresh, onOpen)
                2 -> LittleDownloadsPage(link, onLinkChange, onDownloadLink, onCancelDownload, onPlayLink, isDownloading, transferMessage, onTabSelected)
                else -> OurWatchRoomPage(link, onLinkChange, onPlayLink, roomState, roomError, liveConfig, liveConfigError, liveActive,
                    onStartLive = { liveConfig?.let { config -> roomConnection?.let { room -> room.select("live", config.streamPath, "Live from desktop") } } },
                    voiceState = voiceState, microphoneMessage = microphoneMessage, speakerphoneOn = speakerphoneOn,
                    onCallOrAccept = onCallOrAccept, onDecline = onDecline, onHangUp = onHangUp,
                    onToggleSpeakerphone = onToggleSpeakerphone, onDismissCallError = onDismissCallError, onTabSelected = onTabSelected)
            }
        }
    }
    if (showFolderDialog) {
        AlertDialog(
            onDismissRequest = { showFolderDialog = false },
            title = { Text("A new folder", color = Colors.ink) },
            text = { OutlinedTextField(value = newFolderName, onValueChange = { newFolderName = it }, label = { Text("Folder name") }, singleLine = true, colors = fieldColors()) },
            confirmButton = {
                TextButton(onClick = { val name = newFolderName.trim(); if (name.isNotEmpty()) onCreateFolder(name); newFolderName = ""; showFolderDialog = false }, enabled = newFolderName.isNotBlank()) { Text("Create", color = Colors.primary) }
            },
            dismissButton = { TextButton(onClick = { showFolderDialog = false }) { Text("Cancel", color = Colors.muted) } },
            containerColor = Colors.surface,
        )
    }
}

@Composable
private fun CoupleHomePage(
    user: String, roomState: RoomState?, roomError: String, items: List<MediaEntry>, voiceState: VoiceCallState,
    microphoneMessage: String, speakerphoneOn: Boolean, onTabSelected: (Int) -> Unit, onOpen: (MediaEntry) -> Unit,
    onCallOrAccept: () -> Unit, onDecline: () -> Unit, onHangUp: () -> Unit,
    onToggleSpeakerphone: () -> Unit, onDismissCallError: () -> Unit,
) {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(horizontal = 17.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        item {
            Text("YOUR SHARED SPACE", color = Colors.accentInk, fontSize = 10.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.7.sp)
            Text("A little time for us", color = Colors.ink, fontSize = 29.sp, lineHeight = 35.sp, fontFamily = FontFamily.Serif)
            Text("Hi, $user. What shall we watch tonight?", color = Colors.muted, fontSize = 13.sp, modifier = Modifier.padding(top = 2.dp))
        }
        item {
            Card(colors = CardDefaults.cardColors(containerColor = Colors.surfaceRose), shape = RoundedCornerShape(27.dp)) {
                Column(Modifier.fillMaxWidth().padding(18.dp), verticalArrangement = Arrangement.spacedBy(13.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text("TONIGHT, TOGETHER", color = Colors.accentInk, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.4.sp)
                            Text("Pick a movie.\nMake it a date.", color = Colors.ink, fontSize = 25.sp, lineHeight = 29.sp, fontFamily = FontFamily.Serif, modifier = Modifier.padding(top = 5.dp))
                        }
                        CoupleIllustration(Modifier.size(width = 126.dp, height = 104.dp))
                    }
                    Text(roomState?.title ?: "Your shared shelf is ready when you are.", color = Colors.muted, fontSize = 12.sp, maxLines = 2)
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        CoupleAvatars()
                        Spacer(Modifier.weight(1f))
                        Button(onClick = { onTabSelected(3) }, colors = ButtonDefaults.buttonColors(containerColor = Colors.primary, contentColor = Colors.onPrimary), shape = RoundedCornerShape(15.dp)) {
                            Text("Open our room", fontSize = 12.sp, fontWeight = FontWeight.Bold)
                        }
                    }
                }
            }
        }
        item {
            Card(colors = CardDefaults.cardColors(containerColor = Colors.surfaceSoft), shape = RoundedCornerShape(22.dp)) {
                Row(Modifier.fillMaxWidth().padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text("OUR ROOM", color = Colors.accentInk, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.2.sp)
                        Text(roomState?.title ?: "Ready when you are", color = Colors.ink, fontWeight = FontWeight.SemiBold, fontSize = 16.sp, maxLines = 1, modifier = Modifier.padding(top = 5.dp))
                        Text(when { roomError.isNotBlank() -> roomError; roomState?.playing == true -> "Playing in sync"; roomState?.title != null -> "Paused together"; else -> "Choose a file or play a link for both of you." }, color = Colors.muted, fontSize = 11.sp, maxLines = 2, modifier = Modifier.padding(top = 3.dp))
                    }
                    Text("▷", color = Colors.primary, fontSize = 31.sp, modifier = Modifier.padding(start = 12.dp))
                }
            }
        }
        item { VoiceCallControls(voiceState, microphoneMessage, speakerphoneOn, onCallOrAccept, onDecline, onHangUp, onToggleSpeakerphone, onDismissCallError) }
        item {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) { Text("On our shelf", color = Colors.ink, fontSize = 20.sp, fontWeight = FontWeight.SemiBold); Text("Little favorites you have shared", color = Colors.muted, fontSize = 11.sp) }
                TextButton(onClick = { onTabSelected(1) }) { Text("See all  ›", color = Colors.primary) }
            }
        }
        val previews = items.take(4)
        if (previews.isEmpty()) item {
            Card(colors = CardDefaults.cardColors(containerColor = Colors.surface), shape = RoundedCornerShape(20.dp)) {
                Column(Modifier.fillMaxWidth().padding(18.dp)) { Text("Your shared shelf is waiting", color = Colors.ink, fontWeight = FontWeight.SemiBold); Text("Upload a video or make your first folder.", color = Colors.muted, fontSize = 12.sp, modifier = Modifier.padding(top = 5.dp)); TextButton(onClick = { onTabSelected(1) }) { Text("Open our files", color = Colors.primary) } }
            }
        } else items(previews, key = { it.path }) { entry -> MediaRow(entry) { onOpen(entry) } }
    }
}

@Composable
private fun CoupleAvatars() {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
        PartnerBubble("A", Colors.surfaceRose, Colors.accentInk)
        Text("♡", color = Colors.accent, fontSize = 17.sp, fontWeight = FontWeight.Bold)
        PartnerBubble("E", Colors.surfaceSoft, Colors.primary)
        Text("together", color = Colors.muted, fontSize = 10.sp, modifier = Modifier.padding(start = 2.dp))
    }
}

@Composable
private fun PartnerBubble(initial: String, background: Color, foreground: Color) {
    Box(Modifier.size(37.dp).background(background, CircleShape), contentAlignment = Alignment.Center) {
        Text(initial, color = foreground, fontSize = 13.sp, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun CoupleIllustration(modifier: Modifier = Modifier) {
    val palette = LocalAppPalette.current
    androidx.compose.foundation.Canvas(modifier) {
        val w = size.width
        val h = size.height
        drawCircle(palette.surfaceRose, radius = h * 0.47f, center = androidx.compose.ui.geometry.Offset(w * 0.52f, h * 0.53f))
        val bench = palette.illustrationBench
        drawRoundRect(bench, androidx.compose.ui.geometry.Offset(w * 0.15f, h * 0.59f), androidx.compose.ui.geometry.Size(w * 0.70f, h * 0.045f), androidx.compose.ui.geometry.CornerRadius(h * 0.02f))
        drawRoundRect(bench.copy(alpha = 0.84f), androidx.compose.ui.geometry.Offset(w * 0.19f, h * 0.67f), androidx.compose.ui.geometry.Size(w * 0.62f, h * 0.055f), androidx.compose.ui.geometry.CornerRadius(h * 0.025f))
        drawLine(bench, androidx.compose.ui.geometry.Offset(w * 0.27f, h * 0.70f), androidx.compose.ui.geometry.Offset(w * 0.23f, h * 0.94f), h * 0.035f, cap = androidx.compose.ui.graphics.StrokeCap.Round)
        drawLine(bench, androidx.compose.ui.geometry.Offset(w * 0.73f, h * 0.70f), androidx.compose.ui.geometry.Offset(w * 0.77f, h * 0.94f), h * 0.035f, cap = androidx.compose.ui.graphics.StrokeCap.Round)
        drawLine(bench, androidx.compose.ui.geometry.Offset(w * 0.22f, h * 0.88f), androidx.compose.ui.geometry.Offset(w * 0.78f, h * 0.88f), h * 0.025f, cap = androidx.compose.ui.graphics.StrokeCap.Round)

        val hair = palette.illustrationHair
        val skin = palette.illustrationSkin
        val rose = palette.illustrationRose
        val green = palette.illustrationGreen
        drawCircle(hair, h * 0.105f, androidx.compose.ui.geometry.Offset(w * 0.39f, h * 0.29f))
        drawCircle(skin, h * 0.078f, androidx.compose.ui.geometry.Offset(w * 0.39f, h * 0.30f))
        drawCircle(hair, h * 0.103f, androidx.compose.ui.geometry.Offset(w * 0.62f, h * 0.29f))
        drawCircle(skin, h * 0.078f, androidx.compose.ui.geometry.Offset(w * 0.62f, h * 0.30f))
        drawRoundRect(rose, androidx.compose.ui.geometry.Offset(w * 0.32f, h * 0.39f), androidx.compose.ui.geometry.Size(w * 0.16f, h * 0.24f), androidx.compose.ui.geometry.CornerRadius(h * 0.06f))
        drawRoundRect(green, androidx.compose.ui.geometry.Offset(w * 0.55f, h * 0.39f), androidx.compose.ui.geometry.Size(w * 0.16f, h * 0.24f), androidx.compose.ui.geometry.CornerRadius(h * 0.06f))
        drawLine(skin, androidx.compose.ui.geometry.Offset(w * 0.35f, h * 0.46f), androidx.compose.ui.geometry.Offset(w * 0.53f, h * 0.54f), h * 0.025f, cap = androidx.compose.ui.graphics.StrokeCap.Round)
        drawLine(skin, androidx.compose.ui.geometry.Offset(w * 0.66f, h * 0.46f), androidx.compose.ui.geometry.Offset(w * 0.51f, h * 0.54f), h * 0.025f, cap = androidx.compose.ui.graphics.StrokeCap.Round)
        drawLine(palette.illustrationLine, androidx.compose.ui.geometry.Offset(w * 0.38f, h * 0.62f), androidx.compose.ui.geometry.Offset(w * 0.48f, h * 0.81f), h * 0.028f, cap = androidx.compose.ui.graphics.StrokeCap.Round)
        drawLine(palette.illustrationLine, androidx.compose.ui.geometry.Offset(w * 0.62f, h * 0.62f), androidx.compose.ui.geometry.Offset(w * 0.55f, h * 0.81f), h * 0.028f, cap = androidx.compose.ui.graphics.StrokeCap.Round)
        drawCircle(Color.White, h * 0.014f, androidx.compose.ui.geometry.Offset(w * 0.365f, h * 0.30f))
        drawCircle(Color.White, h * 0.014f, androidx.compose.ui.geometry.Offset(w * 0.595f, h * 0.30f))
    }
}

@Composable
private fun KeepsakesPage(
    folder: String, visible: List<MediaEntry>, itemCount: Int, status: LoadStatus, transferMessage: String,
    isUploading: Boolean, uploadName: String, uploadBytes: Long, uploadTotal: Long?, query: String,
    onQuery: (String) -> Unit, onUpload: () -> Unit, onCreateFolder: () -> Unit,
    onUp: () -> Unit, onRefresh: () -> Unit, onOpen: (MediaEntry) -> Unit,
) {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(horizontal = 17.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            Text("OUR SHARED LIBRARY", color = Colors.accentInk, fontSize = 10.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.5.sp)
            Text(if (folder.isBlank()) "Shared files" else folder.substringAfterLast('/'), color = Colors.ink, fontSize = 27.sp, fontFamily = FontFamily.Serif)
            Text("$itemCount things we can keep together", color = Colors.muted, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
            if (folder.isNotBlank()) TextButton(onClick = onUp, contentPadding = PaddingValues(horizontal = 0.dp, vertical = 4.dp)) { Text("‹  Back to ${folder.substringBeforeLast('/', "Files")}", color = Colors.primary) }
        }
        item {
            Card(colors = CardDefaults.cardColors(containerColor = Colors.surfaceRose), shape = RoundedCornerShape(22.dp)) {
                Column(Modifier.fillMaxWidth().padding(15.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Text("Add a memory to the shelf", color = Colors.ink, fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
                    Text("Upload videos or make a folder for your favorites.", color = Colors.muted, fontSize = 11.sp)
                    Row(horizontalArrangement = Arrangement.spacedBy(9.dp)) {
                        Button(onClick = onUpload, colors = ButtonDefaults.buttonColors(containerColor = Colors.primary, contentColor = Colors.onPrimary), shape = RoundedCornerShape(13.dp), modifier = Modifier.weight(1f)) { Text("↑  Upload", fontWeight = FontWeight.Bold) }
                        OutlinedButton(onClick = onCreateFolder, shape = RoundedCornerShape(13.dp), modifier = Modifier.weight(1f), border = androidx.compose.foundation.BorderStroke(1.dp, Colors.line)) { Text("＋  New folder", color = Colors.ink) }
                    }
                }
            }
        }
        item {
            OutlinedTextField(value = query, onValueChange = onQuery, placeholder = { Text("Find a file or folder") }, modifier = Modifier.fillMaxWidth(), singleLine = true, shape = RoundedCornerShape(15.dp), colors = fieldColors())
        }
        if (isUploading) item {
            Card(colors = CardDefaults.cardColors(containerColor = Colors.surfaceSoft), shape = RoundedCornerShape(16.dp)) {
                Column(Modifier.fillMaxWidth().padding(14.dp)) { Text("Adding $uploadName", color = Colors.ink, fontWeight = FontWeight.SemiBold, fontSize = 12.sp); if (uploadTotal != null && uploadTotal > 0L) LinearProgressIndicator(progress = { (uploadBytes.toFloat() / uploadTotal).coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth().padding(top = 9.dp), color = Colors.success, trackColor = Colors.line) else LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 9.dp), color = Colors.success) }
            }
        } else if (transferMessage.isNotBlank()) item { Text(transferMessage, color = Colors.muted, fontSize = 12.sp) }
        if (status == LoadStatus.Loading) item { LinearProgressIndicator(Modifier.fillMaxWidth(), color = Colors.primary, trackColor = Colors.surfaceRose) }
        if (status is LoadStatus.Error) item { TextButton(onClick = onRefresh) { Text("${status.message} · Tap to try again", color = Colors.danger) } }
        if (visible.isEmpty() && status == LoadStatus.Ready) item {
            Card(colors = CardDefaults.cardColors(containerColor = Colors.surface), shape = RoundedCornerShape(22.dp)) {
                Column(Modifier.fillMaxWidth().padding(vertical = 28.dp, horizontal = 20.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Box(Modifier.size(66.dp).background(Colors.surfaceSoft, CircleShape), contentAlignment = Alignment.Center) { Icon(FolderIcon, contentDescription = null, tint = Colors.primary, modifier = Modifier.size(28.dp)) }
                    Text("A little room to fill", color = Colors.ink, fontSize = 19.sp, fontFamily = FontFamily.Serif, modifier = Modifier.padding(top = 10.dp))
                    Text("Upload something you both love.", color = Colors.muted, fontSize = 12.sp, modifier = Modifier.padding(top = 4.dp))
                }
            }
        } else items(visible, key = { it.path }) { entry -> MediaRow(entry) { onOpen(entry) } }
    }
}

@Composable
private fun LittleDownloadsPage(
    link: String, onLinkChange: (String) -> Unit, onDownload: () -> Unit, onCancel: () -> Unit,
    onPlayLink: () -> Unit, downloading: Boolean, message: String, onTabSelected: (Int) -> Unit,
) {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(horizontal = 17.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        item {
            Text("SAVE A LINK", color = Colors.accentInk, fontSize = 10.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.5.sp)
            Text("Link downloader", color = Colors.ink, fontSize = 27.sp, fontFamily = FontFamily.Serif)
            Text("Save a video or file link to your shared shelf.", color = Colors.muted, fontSize = 12.sp, modifier = Modifier.padding(top = 3.dp))
        }
        item {
            Card(colors = CardDefaults.cardColors(containerColor = Colors.surfaceSoft), shape = RoundedCornerShape(25.dp)) {
                Column(Modifier.fillMaxWidth().padding(17.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) { Column(Modifier.weight(1f)) { Text("Bring a favorite over", color = Colors.ink, fontSize = 18.sp, fontWeight = FontWeight.SemiBold); Text("Paste a direct MP4 or file URL.", color = Colors.muted, fontSize = 11.sp, modifier = Modifier.padding(top = 3.dp)) }; Box(Modifier.size(46.dp).background(Colors.surfaceSoft, CircleShape), contentAlignment = Alignment.Center) { Icon(DownloadIcon, contentDescription = null, tint = Colors.primary, modifier = Modifier.size(24.dp)) } }
                    OutlinedTextField(value = link, onValueChange = onLinkChange, placeholder = { Text("Paste link here") }, singleLine = true, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri), modifier = Modifier.fillMaxWidth(), colors = fieldColors(), shape = RoundedCornerShape(14.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(9.dp)) {
                        Button(onClick = onDownload, enabled = !downloading, modifier = Modifier.weight(1f).height(47.dp), colors = ButtonDefaults.buttonColors(containerColor = Colors.primary, contentColor = Colors.onPrimary), shape = RoundedCornerShape(14.dp)) { Text(if (downloading) "Saving…" else "Download", fontWeight = FontWeight.Bold) }
                        OutlinedButton(onClick = { onPlayLink(); onTabSelected(3) }, enabled = link.isNotBlank(), modifier = Modifier.weight(1f).height(47.dp), shape = RoundedCornerShape(14.dp), border = androidx.compose.foundation.BorderStroke(1.dp, Colors.line)) { Text("Watch together", color = Colors.primary, fontWeight = FontWeight.Bold) }
                    }
                    if (downloading) TextButton(onClick = onCancel, modifier = Modifier.align(Alignment.End)) { Text("Cancel download", color = Colors.danger) }
                }
            }
        }
        if (message.isNotBlank()) item { Card(colors = CardDefaults.cardColors(containerColor = Colors.surfaceSoft), shape = RoundedCornerShape(18.dp)) { Text(message, color = Colors.ink, fontSize = 12.sp, modifier = Modifier.fillMaxWidth().padding(14.dp)) } }
        item {
            Card(colors = CardDefaults.cardColors(containerColor = Colors.surfaceRose), shape = RoundedCornerShape(22.dp)) {
                Row(Modifier.fillMaxWidth().padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) { Text("A file already on your phone?", color = Colors.ink, fontWeight = FontWeight.SemiBold, fontSize = 14.sp); Text("Send local videos to your shared files.", color = Colors.muted, fontSize = 11.sp, modifier = Modifier.padding(top = 4.dp)) }
                    TextButton(onClick = { onTabSelected(1) }) { Text("Upload  ›", color = Colors.primary, fontWeight = FontWeight.Bold) }
                }
            }
        }
    }
}

@Composable
private fun OurWatchRoomPage(
    link: String, onLinkChange: (String) -> Unit, onPlayLink: () -> Unit,
    roomState: RoomState?, roomError: String, liveConfig: LiveConfig?, liveConfigError: String,
    liveActive: Boolean, onStartLive: () -> Unit,
    voiceState: VoiceCallState, microphoneMessage: String, speakerphoneOn: Boolean, onCallOrAccept: () -> Unit,
    onDecline: () -> Unit, onHangUp: () -> Unit, onToggleSpeakerphone: () -> Unit, onDismissCallError: () -> Unit,
    onTabSelected: (Int) -> Unit,
) {
    val context = LocalContext.current
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(horizontal = 17.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(13.dp)) {
        item {
            Text("PRESS PLAY, BE TOGETHER", color = Colors.accentInk, fontSize = 10.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.5.sp)
            Text("Our watch room", color = Colors.ink, fontSize = 27.sp, fontFamily = FontFamily.Serif)
        }
        item {
            Card(colors = CardDefaults.cardColors(containerColor = Colors.surfaceRose), shape = RoundedCornerShape(24.dp)) {
                Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(9.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) { Column(Modifier.weight(1f)) { Text("PLAY FOR BOTH OF US", color = Colors.accentInk, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.2.sp); Text(roomState?.title ?: "Choose tonight’s feature", color = Colors.ink, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, modifier = Modifier.padding(top = 4.dp)) }; CoupleAvatars() }
                    OutlinedTextField(value = link, onValueChange = onLinkChange, placeholder = { Text("Paste a supported video link") }, singleLine = true, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri), modifier = Modifier.fillMaxWidth(), colors = fieldColors(), shape = RoundedCornerShape(14.dp))
                    Button(onClick = onPlayLink, enabled = link.isNotBlank(), modifier = Modifier.fillMaxWidth().height(47.dp), colors = ButtonDefaults.buttonColors(containerColor = Colors.primary, contentColor = Colors.onPrimary), shape = RoundedCornerShape(14.dp)) { Text("Play together", fontWeight = FontWeight.Bold) }
                    Text(roomError.ifBlank { if (roomState?.playing == true) "Playing in sync" else "Direct media and supported provider links are shared in the room." }, color = Colors.muted, fontSize = 11.sp)
                    TextButton(onClick = { onTabSelected(1) }, contentPadding = PaddingValues(0.dp)) { Text("Browse our files  ›", color = Colors.primary) }
                }
            }
        }
        item { VoiceCallControls(voiceState, microphoneMessage, speakerphoneOn, onCallOrAccept, onDecline, onHangUp, onToggleSpeakerphone, onDismissCallError) }
        item {
            Card(colors = CardDefaults.cardColors(containerColor = Colors.surfaceSoft), shape = RoundedCornerShape(23.dp)) {
                Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("DESKTOP STREAM", color = Colors.primary, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.2.sp)
                    Text("A little window into your desktop", color = Colors.ink, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                    Text("Go live in OBS or Streamlabs; your partner watches the feed here.", color = Colors.muted, fontSize = 11.sp)
                    if (liveConfig == null) {
                        Text(liveConfigError.ifBlank { "Desktop streaming setup is unavailable." }, color = Colors.muted, fontSize = 11.sp)
                    } else {
                        Text(if (liveActive) "●  Desktop is live" else "○  Waiting for the desktop stream", color = if (liveActive) Colors.success else Colors.muted, fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
                        Text("OBS Studio · WHIP", color = Colors.ink, fontSize = 11.sp, fontWeight = FontWeight.Bold)
                        CopyStreamValue("Server URL", liveConfig.whipUrl, context)
                        Text("Streamlabs · Custom RTMP", color = Colors.ink, fontSize = 11.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(top = 3.dp))
                        CopyStreamValue("Server URL", liveConfig.rtmpsServerUrl, context)
                        CopyStreamValue("Stream key", liveConfig.streamKey, context)
                        Button(onClick = onStartLive, enabled = liveActive, modifier = Modifier.fillMaxWidth().height(46.dp), colors = ButtonDefaults.buttonColors(containerColor = Colors.primary, contentColor = Colors.onPrimary), shape = RoundedCornerShape(14.dp)) { Text("Watch desktop live", fontWeight = FontWeight.Bold) }
                    }
                }
            }
        }
    }
}

@Composable
@OptIn(UnstableApi::class)
private fun PlayerScreen(
    video: PlaybackSource,
    roomState: RoomState?,
    localControlPendingUntil: AtomicLong,
    onPlayback: (Boolean, Long) -> Unit,
    onSeek: (Long) -> Unit,
    onBack: () -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val window = context.findActivity()?.window
    val url = video.url
    var playbackError by remember(url) { mutableStateOf<String?>(null) }
    var isFullscreen by remember(url) { mutableStateOf(false) }
    val latestRoomState = rememberUpdatedState(roomState)
    val player = remember(url, video.audioUrl, video.authHeader, video.requestHeaders) {
        val http = DefaultHttpDataSource.Factory()
        val requestHeaders = buildMap {
            putAll(video.requestHeaders)
            video.authHeader?.let { put("Authorization", it) }
        }
        if (requestHeaders.isNotEmpty()) http.setDefaultRequestProperties(requestHeaders)
        val dataSource = DefaultDataSource.Factory(context, http)
        val uri = Uri.parse(url)
        val mediaPath = (uri.getQueryParameter("path") ?: uri.path.orEmpty()).lowercase()
        val item = PlayerItem.Builder().setUri(uri).apply {
            when {
                mediaPath.endsWith(".m3u8") -> setMimeType(MimeTypes.APPLICATION_M3U8)
                mediaPath.endsWith(".mpd") -> setMimeType(MimeTypes.APPLICATION_MPD)
            }
        }.setSubtitleConfigurations(
            video.subtitles.mapIndexed { index, subtitle ->
                PlayerItem.SubtitleConfiguration.Builder(Uri.parse(subtitle.url))
                    .setMimeType(subtitle.mimeType)
                    .setLanguage("und")
                    .setLabel(subtitle.label)
                    .setSelectionFlags(if (index == 0) C.SELECTION_FLAG_DEFAULT else 0)
                    .build()
            },
        ).build()
        val sourceFactory = DefaultMediaSourceFactory(dataSource)
        val videoSource = sourceFactory.createMediaSource(item)
        val mediaSource = video.audioUrl?.let { audioUrl ->
            MergingMediaSource(videoSource, sourceFactory.createMediaSource(PlayerItem.fromUri(audioUrl)))
        } ?: videoSource
        ExoPlayer.Builder(context).setMediaSourceFactory(sourceFactory).build().apply {
            setMediaSource(mediaSource)
            prepare()
            playWhenReady = video.playing
        }
    }
    val roomSeekTargetMs = remember(player) { AtomicLong(-1L) }
    val roomSeekExpiresAt = remember(player) { AtomicLong(0L) }
    LaunchedEffect(player) {
        var appliedRevision = Long.MIN_VALUE
        while (true) {
            val state = latestRoomState.value
            val nowElapsed = SystemClock.elapsedRealtime()
            if (state?.source != null && localControlPendingUntil.get() <= nowElapsed) {
                if (video.isLive || state.sourceKind == "live") {
                    if (player.playWhenReady != state.playing) player.playWhenReady = state.playing
                    appliedRevision = state.revision
                    delay(300L)
                    continue
                }
                val elapsed = if (state.playing) {
                    (nowElapsed - state.receivedElapsedMs).coerceAtLeast(0L)
                } else 0L
                val target = state.positionMs.saturatingAdd(elapsed)
                val actual = player.currentPosition.coerceAtLeast(0L)
                val drift = target - actual

                if (appliedRevision != state.revision) {
                    if (player.playWhenReady != state.playing) player.playWhenReady = state.playing
                    if (!state.playing) player.playbackParameters = PlaybackParameters.DEFAULT
                    if (kotlin.math.abs(drift) > 250L) {
                        roomSeekTargetMs.set(target)
                        roomSeekExpiresAt.set(nowElapsed + 2_500L)
                        player.seekTo(target)
                    }
                    appliedRevision = state.revision
                } else if (player.playbackState == Player.STATE_READY) {
                    if (state.playing) {
                        when {
                            kotlin.math.abs(drift) > 650L -> {
                                roomSeekTargetMs.set(target)
                                roomSeekExpiresAt.set(nowElapsed + 2_500L)
                                player.seekTo(target)
                                player.playbackParameters = PlaybackParameters.DEFAULT
                            }
                            drift > 90L -> player.playbackParameters = PlaybackParameters(1.04f)
                            drift < -90L -> player.playbackParameters = PlaybackParameters(0.96f)
                            else -> player.playbackParameters = PlaybackParameters.DEFAULT
                        }
                    } else {
                        player.playWhenReady = false
                        player.playbackParameters = PlaybackParameters.DEFAULT
                        if (kotlin.math.abs(drift) > 250L) {
                            roomSeekTargetMs.set(target)
                            roomSeekExpiresAt.set(nowElapsed + 2_500L)
                            player.seekTo(target)
                        }
                    }
                }
            }
            delay(300L)
        }
    }
    DisposableEffect(player) {
        val listener = object : Player.Listener {
            override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                if (latestRoomState.value?.playing == playWhenReady) return
                if (localControlPendingUntil.get() > SystemClock.elapsedRealtime()) return
                onPlayback(playWhenReady, player.currentPosition.coerceAtLeast(0L))
            }

            override fun onPositionDiscontinuity(
                oldPosition: Player.PositionInfo,
                newPosition: Player.PositionInfo,
                reason: Int,
            ) {
                if (reason == Player.DISCONTINUITY_REASON_SEEK) {
                    val now = SystemClock.elapsedRealtime()
                    val roomTarget = roomSeekTargetMs.get()
                    if (roomTarget >= 0L && now <= roomSeekExpiresAt.get() &&
                        kotlin.math.abs(newPosition.positionMs - roomTarget) <= 4_000L
                    ) {
                        roomSeekTargetMs.compareAndSet(roomTarget, -1L)
                        return
                    }
                    onSeek(newPosition.positionMs.coerceAtLeast(0L))
                }
            }

            override fun onPlayerError(error: PlaybackException) {
                playbackError = "This link did not provide a playable video stream. Try a direct MP4 or HLS URL."
            }
        }
        player.addListener(listener)
        onDispose { player.removeListener(listener); player.release() }
    }

    BackHandler(enabled = isFullscreen) { isFullscreen = false }
    DisposableEffect(window, isFullscreen) {
        val controller = window?.let { WindowCompat.getInsetsController(it, it.decorView) }
        if (isFullscreen) {
            controller?.systemBarsBehavior =
                androidx.core.view.WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            controller?.hide(WindowInsetsCompat.Type.systemBars())
        } else {
            controller?.show(WindowInsetsCompat.Type.systemBars())
        }
        onDispose { controller?.show(WindowInsetsCompat.Type.systemBars()) }
    }

        if (isFullscreen) {
            Box(Modifier.fillMaxSize().background(Colors.videoBackdrop)) {
            NativeVideoView(player, Modifier.fillMaxSize(), video.isLive)
            if (video.isLive) LiveBadge(Modifier.align(Alignment.TopEnd).padding(top = 14.dp, end = 62.dp))
            FullscreenToggle(
                expanded = true,
                onClick = { isFullscreen = false },
                modifier = Modifier.align(Alignment.TopStart).padding(12.dp),
            )
        }
    } else {
        Column(Modifier.fillMaxSize().background(Colors.canvas)) {
            TextButton(onClick = onBack, modifier = Modifier.padding(start = 12.dp, top = 12.dp)) {
                Text("‹  Back", color = Colors.primary)
            }
            Text(video.title, color = Colors.ink, fontSize = 21.sp, fontFamily = FontFamily.Serif,
                modifier = Modifier.padding(horizontal = 22.dp, vertical = 12.dp))
            Text(if (video.isLive) "Live desktop · shared with your room" else "Synced room playback", color = Colors.success, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 22.dp, vertical = 4.dp))
            Box(Modifier.fillMaxWidth().aspectRatio(16f / 9f)) {
                NativeVideoView(player, Modifier.fillMaxSize(), video.isLive)
                if (video.isLive) LiveBadge(Modifier.align(Alignment.TopStart).padding(10.dp))
                FullscreenToggle(
                    expanded = false,
                    onClick = { isFullscreen = true },
                    modifier = Modifier.align(Alignment.TopEnd).padding(8.dp),
                )
            }
            TextButton(
                onClick = {
                    TrackSelectionDialogBuilder(context, "Subtitles", player, C.TRACK_TYPE_TEXT)
                        .setShowDisableOption(true)
                        .build()
                        .show()
                },
                enabled = !video.isLive,
                modifier = Modifier.padding(horizontal = 12.dp),
            ) {
                Text("CC  Subtitles", color = Colors.primary)
            }
            if (playbackError != null) {
                Text(playbackError!!, color = Colors.danger, fontSize = 13.sp, modifier = Modifier.padding(18.dp))
            } else {
                Text("Playback uses the device’s native media codecs.", color = Colors.muted, fontSize = 12.sp, modifier = Modifier.padding(18.dp))
            }
        }
    }
}

@Composable
private fun NativeVideoView(player: ExoPlayer, modifier: Modifier = Modifier, isLive: Boolean = false) {
    AndroidView(
        factory = { context -> PlayerView(context).apply { useController = !isLive; this.player = player } },
        update = { it.useController = !isLive; it.player = player },
        modifier = modifier,
    )
}

@Composable
private fun LiveBadge(modifier: Modifier = Modifier) {
    Row(
        modifier.background(Colors.danger, RoundedCornerShape(8.dp)).padding(horizontal = 9.dp, vertical = 5.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        Box(Modifier.size(6.dp).background(Colors.onPrimary, CircleShape))
        Text("LIVE", color = Colors.onPrimary, fontSize = 10.sp, fontWeight = FontWeight.Bold, letterSpacing = 0.8.sp)
    }
}

@Composable
private fun FullscreenToggle(expanded: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
    IconButton(
        onClick = onClick,
        modifier = modifier
            .background(Colors.videoBackdrop.copy(alpha = 0.68f), CircleShape)
            .semantics { contentDescription = if (expanded) "Exit full screen" else "Enter full screen" },
    ) {
        Text(if (expanded) "⤢" else "⛶", color = Color.White, fontSize = 22.sp, fontWeight = FontWeight.Bold)
    }
}

private fun Long.saturatingAdd(value: Long): Long =
    if (value > 0L && this > Long.MAX_VALUE - value) Long.MAX_VALUE else this + value

@Composable
private fun FormField(label: String, value: String, onChange: (String) -> Unit, hint: String) {
    OutlinedTextField(
        value = value, onValueChange = onChange, label = { Text(label) }, placeholder = { Text(hint) },
        modifier = Modifier.fillMaxWidth(), singleLine = true,
        keyboardOptions = if (label.contains("address")) KeyboardOptions(keyboardType = KeyboardType.Uri) else KeyboardOptions.Default,
        colors = fieldColors(),
    )
}

@Composable
private fun fieldColors() = OutlinedTextFieldDefaults.colors(
    focusedBorderColor = Colors.primary, unfocusedBorderColor = Colors.line,
    focusedLabelColor = Colors.primary, unfocusedLabelColor = Colors.muted,
    focusedTextColor = Colors.ink, unfocusedTextColor = Colors.ink, cursorColor = Colors.success,
)

private fun requiresProviderResolution(value: String): Boolean {
    val host = Uri.parse(value).host?.lowercase()?.trimEnd('.').orEmpty()
    return host == "youtu.be" || host == "youtube.com" || host.endsWith(".youtube.com") ||
        host == "vimeo.com" || host.endsWith(".vimeo.com")
}

private fun formatSize(bytes: Long): String = when {
    bytes >= 1_000_000_000 -> String.format("%.1f GB", bytes / 1_000_000_000.0)
    bytes >= 1_000_000 -> String.format("%.0f MB", bytes / 1_000_000.0)
    bytes >= 1_000 -> String.format("%.0f KB", bytes / 1_000.0)
    else -> bytes.toString() + " B"
}
