// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

// CI-only: packages pre-built JNI libs + Kotlin bindings into AAR for Maven.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("maven-publish")
}

val libVersion: String = System.getenv("VERSION") ?: "0.1.0"

android {
    namespace = "app.vauchi.platform"
    compileSdk = 35

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    // JNI libraries are copied from the pre-built bindings zip
    sourceSets {
        getByName("main") {
            jniLibs.srcDir("src/main/jniLibs")
        }
    }

    publishing {
        singleVariant("release") {
            withSourcesJar()
        }
    }
}

dependencies {
    implementation("net.java.dev.jna:jna:5.14.0@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.7.3")
}

publishing {
    publications {
        register<MavenPublication>("release") {
            groupId = "app.vauchi"
            artifactId = "vauchi-platform"
            version = libVersion

            afterEvaluate {
                from(components["release"])
            }

            pom {
                name.set("Vauchi Platform")
                description.set("UniFFI bindings for Vauchi core on Android")
                url.set("https://gitlab.com/vauchi/core")

                licenses {
                    license {
                        name.set("GPL-3.0-or-later")
                        url.set("https://www.gnu.org/licenses/gpl-3.0.html")
                    }
                }

                developers {
                    developer {
                        name.set("Vauchi Contributors")
                    }
                }

                scm {
                    connection.set("scm:git:git://gitlab.com/vauchi/core.git")
                    developerConnection.set("scm:git:ssh://gitlab.com/vauchi/core.git")
                    url.set("https://gitlab.com/vauchi/core")
                }
            }
        }
    }

    repositories {
        maven {
            name = "GitLab"
            // In CI: publishes to core project's Maven registry (visible at group level)
            // Locally: falls back to vauchi-platform-kotlin's registry (for testing)
            url = uri(
                System.getenv("CI_API_V4_URL")?.let {
                    "$it/projects/${System.getenv("CI_PROJECT_ID")}/packages/maven"
                } ?: "https://gitlab.com/api/v4/projects/77955319/packages/maven"
            )

            credentials(HttpHeaderCredentials::class) {
                name = if (System.getenv("CI_JOB_TOKEN") != null) "Job-Token" else "Private-Token"
                value = System.getenv("CI_JOB_TOKEN") ?: System.getenv("GITLAB_TOKEN") ?: ""
            }

            authentication {
                create<HttpHeaderAuthentication>("header")
            }
        }
    }
}
