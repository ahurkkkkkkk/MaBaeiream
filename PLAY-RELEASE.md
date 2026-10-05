# Android Play release notes

This is a release checklist for the native Android client, not a claim that the app has already passed Play review.

## Current Android configuration

- `artifacts/android/mabaeiream-android-0.3.7-sideload-signed-r15.apk` is the current installable sideload build, signed with the same workspace Android debug certificate as the previous sideload APK. Do not upload it to Play; sign the release AAB with the app owner's upload key.
- Native Kotlin with Jetpack Compose; native Media3 playback; no WebView-based app shell.
- `compileSdk 37` and `targetSdk 36`. Google Play requires new phone/tablet apps and updates to target API 36 or higher beginning August 31, 2026. Compose's current stable libraries require compile SDK 37.
- The merged APK includes `INTERNET`, `MODIFY_AUDIO_SETTINGS`, `RECORD_AUDIO`, `ACCESS_NETWORK_STATE`, `WAKE_LOCK`, and AndroidX's app-scoped dynamic-receiver permission. Android requests microphone access only after the user taps Call or Answer; denial leaves library browsing and playback available.
- No broad storage, media-library, location, or notification permission is declared. The library and imported files stay on the server.
- Cleartext traffic is disabled; sign-in requires HTTPS with normal Android certificate validation.
- This diagnostic release keeps R8 minification disabled while validating HyperOS compatibility. It is not the final Play release build.
- No analytics or advertising SDK is configured.

## Before upload

1. Confirm you control the `ahura.site` domain and that `site.ahura.mabaeiream` is the package ID you want to keep before its first Play upload; changing an app ID after publication creates a different Play app.
2. Build and sign an Android App Bundle using your own upload key. Keep the signing key and passwords outside this repository.
3. Host a final privacy policy at a public URL and enter it in Play Console. The policy must match the exact server deployment and its proxy/logging configuration.
4. Complete the Data safety form from the final app and server behavior. The app sends the account name and password to the configured server over HTTPS; the server stores a password hash and bearer-session hashes. Media names, direct-download links, and supported YouTube/Vimeo URLs are processed by the private server. Android streams resolved provider media from the provider CDN.
5. Supply Play's app access instructions for the private account-gated service, plus accurate screenshots, content rating, target audience, and distribution details.
6. Build and inspect a release AAB with the current Play Console checks. Play acceptance also depends on the account, listing, policy declarations, signing, testing, and actual production behavior.

## Voice-call permissions

Voice calls are audio-only and begin only after an explicit Call or Answer action. The microphone permission appears at that point. Calls stop when the app leaves the foreground, so this version does not request microphone foreground-service permissions or continue recording in the background.

Before submitting, test permission grant and denial, call/answer/decline/hang-up, audio focus, headset/Bluetooth routing, and Android-to-desktop calls on physical devices. The service now issues short-lived authenticated TURN credentials and the clients use coturn on `ahura.site` as a relay fallback; cross-network call quality still needs physical-device validation. Android uses libwebrtc's built-in echo and noise processing rather than device-specific hardware effects.
