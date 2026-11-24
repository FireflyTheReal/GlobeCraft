plugins {
    id("com.android.application")
}

android {
    namespace = "app.globecraft.mobile"
    compileSdk = 36
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "app.globecraft.mobile"
        minSdk = 30
        targetSdk = 36
        versionCode = 1
        versionName = "0.4.0-phase0"
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Sideloading: signed with the debug key until a release key exists.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
