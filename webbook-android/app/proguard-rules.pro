# ProGuard rules for WebBook

# Keep UniFFI generated classes
-keep class uniffi.** { *; }
-keep class com.webbook.uniffi.** { *; }

# Keep JNA classes
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }

# Keep native library loading
-keepclassmembers class * {
    native <methods>;
}
