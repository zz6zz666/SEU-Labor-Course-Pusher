import org.gradle.api.tasks.Exec
import java.util.Properties

plugins {
    id("com.android.application")
}

val keystorePropsFile = rootProject.file("keystore.properties")
val keystoreProps = Properties().apply {
    if (keystorePropsFile.isFile) {
        keystorePropsFile.inputStream().use { load(it) }
    }
}
val defaultReleaseStore = File(
    System.getProperty("user.home"),
    ".android/seu-labor-release.keystore"
)
val configuredReleaseStore = keystoreProps.getProperty("storeFile")
    ?.takeIf { it.isNotBlank() }
    ?.let { rootProject.file(it) }
val releaseStoreFile = configuredReleaseStore ?: defaultReleaseStore
val releaseStorePassword = keystoreProps.getProperty("storePassword")
val releaseKeyAlias = keystoreProps.getProperty("keyAlias")
val hasReleaseSigning = releaseStoreFile.isFile &&
    !releaseStorePassword.isNullOrBlank() &&
    !releaseKeyAlias.isNullOrBlank()

android {
    namespace = "com.zz6zz666.seulabor"
    compileSdk = 36
    buildToolsVersion = "36.1.0"
    ndkVersion = "27.0.12077973"

    defaultConfig {
        applicationId = "com.zz6zz666.seulabor"
        minSdk = 26
        targetSdk = 35
        versionCode = 2
        versionName = "1.2.1"

        // Mainstream 64-bit phones only; add x86_64 locally if you need an emulator.
        ndk {
            abiFilters.add("arm64-v8a")
        }
    }

    signingConfigs {
        if (hasReleaseSigning) {
            create("release") {
                storeFile = releaseStoreFile
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = keystoreProps.getProperty("keyPassword")
                    ?.takeIf { it.isNotBlank() }
                    ?: releaseStorePassword
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            signingConfig = signingConfigs.findByName("release")
                ?: signingConfigs.getByName("debug")
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
        debug {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.work:work-runtime-ktx:2.9.1")
}

val cargoExecutable = if (System.getProperty("os.name").startsWith("Windows")) {
    "${System.getenv("USERPROFILE")}\\.cargo\\bin\\cargo.exe"
} else {
    "cargo"
}

val cargoBuild = tasks.register<Exec>("cargoBuild") {
    workingDir = rootProject.projectDir.parentFile
    environment("RUSTFLAGS", "-C link-arg=-Wl,-z,max-page-size=16384")
    val cargoBin = File(System.getProperty("user.home"), ".cargo/bin").absolutePath
    environment("PATH", "$cargoBin${File.pathSeparator}${System.getenv("PATH")}")
    commandLine(
        cargoExecutable,
        "ndk",
        "-t", "arm64-v8a",
        "-o", "${projectDir}/src/main/jniLibs",
        "build",
        "--profile", "android_release",
        "-p", "seu-labor-core",
        "--no-default-features",
        "--features", "rustls,android-ffi"
    )
}

tasks.named("preBuild") {
    dependsOn(cargoBuild)
}
