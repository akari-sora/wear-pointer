plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// Emulator-only BLE central that logs every pointer notification.
// Not shipped. See docs/emulator-harness.md.
android {
    namespace = "dev.wearpointer.probe"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.wearpointer.probe"
        minSdk = 33
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}
