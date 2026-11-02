import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import groovy.json.JsonOutput
import java.security.MessageDigest
import org.gradle.api.DefaultTask
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// A standalone task stores only Gradle managed properties, so configuration-cache replay does
// not serialize references to this build script or Project.
abstract class GenerateClientAssets : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val sources: ConfigurableFileCollection

    @get:OutputDirectory
    abstract val destinationDirectory: DirectoryProperty

    @TaskAction
    fun generate() {
        val destination = destinationDirectory.get().dir("client").asFile
        destination.mkdirs()
        val entries = sortedMapOf<String, Map<String, Any>>()
        fun digest(bytes: ByteArray) = MessageDigest.getInstance("SHA-256").digest(bytes)
            .joinToString("") { "%02x".format(it) }
        sources.files.sortedBy { it.name }.forEach { source ->
            // Match the release packager and runtime's content-addressed manifest.
            val bytes = source.readBytes()
            source.copyTo(destination.resolve(source.name), overwrite = true)
            entries[source.name] = mapOf("sha256" to digest(bytes), "size" to bytes.size)
        }
        val version = digest(entries.entries.joinToString("") { (name, file) ->
            "$name\u0000${file["sha256"]}\u0000${file["size"]}\n"
        }.toByteArray(Charsets.UTF_8))
        destination.resolve("manifest.json").writeText(JsonOutput.toJson(mapOf(
            "format" to 1, "version" to version, "api_contract" to 1, "min_native" to 1,
            "files" to entries
        )))
    }
}

// The native client ships the same sources as the server, never a separately maintained UI.
val generatedClientAssets = layout.buildDirectory.dir("generated/clientAssets")
val generateClientAssets = tasks.register<GenerateClientAssets>("generateClientAssets") {
    sources.from(fileTree(rootProject.file("../../../../web")) {
        include("*.js", "*.css", "*.svg", "index.html", "og.jpg")
    })
    destinationDirectory.set(generatedClientAssets)
}

android {
    compileSdk = 36
    namespace = "com.agentshangout.android"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "true"
        applicationId = "com.agentshangout.android"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
        val serverUrl = System.getenv("AGENTSHANGOUT_URL") ?: "https://agentshangout.com"
        buildConfigField("String", "AGENTSHANGOUT_URL", JsonOutput.toJson(serverUrl))
        // Public Firebase Android configuration is supplied by the release builder, never checked in.
        mapOf("APP_ID" to "google_app_id", "PROJECT_ID" to "project_id",
            "API_KEY" to "google_api_key", "SENDER_ID" to "gcm_defaultSenderId").forEach { (key, resource) ->
            val value = providers.environmentVariable("AGENTSHANGOUT_FIREBASE_$key").orElse("").get()
            buildConfigField("String", "FIREBASE_$key", JsonOutput.toJson(value))
            resValue("string", resource, value)
        }
    }
    sourceSets.getByName("main").assets.srcDir(generatedClientAssets.get().asFile)
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
        resValues = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation(platform("com.google.firebase:firebase-bom:35.1.0"))
    implementation("com.google.firebase:firebase-messaging")
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

tasks.named("preBuild").configure { dependsOn(generateClientAssets) }

apply(from = file("tauri.build.gradle.kts"))
