plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "site.ahura.mabaeiream"
    compileSdk = 37
    buildToolsVersion = "37.0.0"

    defaultConfig {
        applicationId = "site.ahura.mabaeiream"
        minSdk = 26
        targetSdk = 36
        versionCode = 13
        versionName = "0.3.10"
    }

    buildFeatures { compose = true }

    buildTypes {
        release {
            // Keep the sideload build free of bytecode obfuscation while diagnosing
            // HyperOS's Android compatibility crash reported on release builds.
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.09.00"))
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.4")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.media3:media3-exoplayer:1.11.1")
    implementation("androidx.media3:media3-exoplayer-hls:1.11.1")
    implementation("androidx.media3:media3-exoplayer-dash:1.11.1")
    implementation("androidx.media3:media3-ui:1.11.1")
    implementation("com.squareup.okhttp3:okhttp:5.4.0")
    implementation("io.github.webrtc-sdk:android:150.7871.01")
    debugImplementation("androidx.compose.ui:ui-tooling")
}
