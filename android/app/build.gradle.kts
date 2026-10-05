plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}
android {
    namespace = "com.motionstudio.editor"
    compileSdk = 35
    defaultConfig {
        applicationId = "com.motionstudio.editor"
        minSdk = 29
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    buildFeatures { compose = true }
    sourceSets.getByName("androidTest").assets.srcDir("../../crates/aem-media/tests/fixtures")
    androidResources { noCompress += listOf("wav", "mp3", "m4a", "mp4", "bin") }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildTypes {
        debug { isDebuggable = true }
        release { isMinifyEnabled = false }
        create("preview") {
            initWith(getByName("release"))
            isDebuggable=false
            signingConfig=signingConfigs.getByName("debug")
            versionNameSuffix="-preview.1"
            matchingFallbacks+=listOf("release")
        }
        create("benchmark") {
            initWith(getByName("release"))
            isDebuggable=false
            signingConfig=signingConfigs.getByName("debug")
            matchingFallbacks+=listOf("release")
        }
    }
    testBuildType=if(providers.gradleProperty("performanceTest").orNull=="true") "benchmark" else "debug"
}
dependencies {
    implementation(platform("androidx.compose:compose-bom:2025.04.01"))
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    debugImplementation("androidx.compose.ui:ui-tooling")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation(platform("androidx.compose:compose-bom:2025.04.01"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test.uiautomator:uiautomator:2.4.0")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
