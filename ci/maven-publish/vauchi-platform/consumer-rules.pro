# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later

# Consumer ProGuard rules for VauchiPlatform
# These rules are automatically included when the library is used

# Keep UniFFI generated classes
-keep class uniffi.vauchi_platform.** { *; }

# Keep JNA classes
-dontwarn com.sun.jna.**
-keep class com.sun.jna.** { *; }
