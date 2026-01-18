package com.vauchi.util

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.widget.Toast
import uniffi.vauchi_mobile.MobileContactField
import uniffi.vauchi_mobile.MobileFieldType

/**
 * Utility object for opening contact fields in external applications.
 */
object ContactActions {

    /**
     * Allowed URI schemes for security.
     */
    private val ALLOWED_SCHEMES = setOf("tel", "mailto", "sms", "https", "http", "geo")

    /**
     * Social network URL templates.
     */
    private val SOCIAL_URL_TEMPLATES = mapOf(
        "twitter" to "https://twitter.com/{username}",
        "x" to "https://twitter.com/{username}",
        "github" to "https://github.com/{username}",
        "linkedin" to "https://linkedin.com/{username}",
        "instagram" to "https://instagram.com/{username}",
        "facebook" to "https://facebook.com/{username}",
        "mastodon" to "https://mastodon.social/@{username}",
        "youtube" to "https://youtube.com/@{username}",
        "tiktok" to "https://tiktok.com/@{username}",
        "reddit" to "https://reddit.com/u/{username}",
        "bluesky" to "https://bsky.app/profile/{username}"
    )

    /**
     * Opens a contact field in the appropriate external application.
     *
     * @param context The Android context
     * @param field The contact field to open
     * @return true if the field was opened, false if it was copied to clipboard
     */
    fun openField(context: Context, field: MobileContactField): Boolean {
        val uri = fieldToUri(field)

        if (uri == null) {
            copyToClipboard(context, field.value, field.label)
            return false
        }

        // Security check: validate URI scheme
        val scheme = uri.scheme?.lowercase()
        if (scheme != null && scheme !in ALLOWED_SCHEMES) {
            Toast.makeText(context, "Cannot open: blocked for security", Toast.LENGTH_SHORT).show()
            copyToClipboard(context, field.value, field.label)
            return false
        }

        val intent = Intent(Intent.ACTION_VIEW, uri)

        return try {
            if (intent.resolveActivity(context.packageManager) != null) {
                context.startActivity(intent)
                true
            } else {
                Toast.makeText(context, "No app available to open this", Toast.LENGTH_SHORT).show()
                copyToClipboard(context, field.value, field.label)
                false
            }
        } catch (e: Exception) {
            Toast.makeText(context, "Failed to open: ${e.message}", Toast.LENGTH_SHORT).show()
            copyToClipboard(context, field.value, field.label)
            false
        }
    }

    /**
     * Converts a contact field to a URI.
     */
    private fun fieldToUri(field: MobileContactField): Uri? {
        val value = field.value.trim()
        if (value.isEmpty()) return null

        return when (field.fieldType) {
            MobileFieldType.PHONE -> Uri.parse("tel:$value")
            MobileFieldType.EMAIL -> Uri.parse("mailto:$value")
            MobileFieldType.WEBSITE -> websiteToUri(value)
            MobileFieldType.ADDRESS -> Uri.parse("geo:0,0?q=${Uri.encode(value)}")
            MobileFieldType.SOCIAL -> socialToUri(field.label, value)
            MobileFieldType.CUSTOM -> detectAndConvert(value)
        }
    }

    /**
     * Converts a website value to a URI, adding https:// if needed.
     */
    private fun websiteToUri(value: String): Uri? {
        // Check for blocked schemes
        val blockedSchemes = setOf("javascript", "vbscript", "data", "file")
        val scheme = value.substringBefore("://").lowercase()
        if (scheme in blockedSchemes) return null

        return when {
            value.startsWith("https://") || value.startsWith("http://") -> Uri.parse(value)
            value.contains("://") -> null // Unknown scheme
            else -> Uri.parse("https://$value")
        }
    }

    /**
     * Converts a social media field to a profile URL.
     */
    private fun socialToUri(label: String, value: String): Uri? {
        val network = label.lowercase()
        val template = SOCIAL_URL_TEMPLATES[network] ?: return null

        // Normalize username (remove @ prefix)
        var username = value.trimStart('@')

        // LinkedIn special handling
        if (network == "linkedin" && !username.startsWith("in/")) {
            username = "in/$username"
        }

        val url = template.replace("{username}", username)
        return Uri.parse(url)
    }

    /**
     * Detects the type of value and converts to appropriate URI.
     * Used for Custom fields.
     */
    private fun detectAndConvert(value: String): Uri? {
        return when {
            // URL pattern
            value.startsWith("https://") || value.startsWith("http://") -> Uri.parse(value)

            // Email pattern
            value.contains("@") && value.contains(".") && !value.contains(" ") ->
                Uri.parse("mailto:$value")

            // Phone pattern (has enough digits)
            value.count { it.isDigit() } >= 7 &&
                value.all { it.isDigit() || it in " -+()./" } ->
                Uri.parse("tel:$value")

            // No pattern detected
            else -> null
        }
    }

    /**
     * Copies a value to the clipboard.
     */
    private fun copyToClipboard(context: Context, value: String, label: String) {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
        val clip = android.content.ClipData.newPlainText(label, value)
        clipboard.setPrimaryClip(clip)
        Toast.makeText(context, "Copied to clipboard", Toast.LENGTH_SHORT).show()
    }

    /**
     * Returns an icon for a field type.
     */
    fun getFieldIcon(fieldType: MobileFieldType): String {
        return when (fieldType) {
            MobileFieldType.PHONE -> "\uD83D\uDCDE"  // Phone
            MobileFieldType.EMAIL -> "\u2709\uFE0F"  // Envelope
            MobileFieldType.WEBSITE -> "\uD83C\uDF10" // Globe
            MobileFieldType.ADDRESS -> "\uD83D\uDCCD" // Pin
            MobileFieldType.SOCIAL -> "\uD83D\uDC64"  // Person
            MobileFieldType.CUSTOM -> "\uD83D\uDCCB"  // Clipboard
        }
    }

    /**
     * Returns a description of the action for a field type.
     */
    fun getActionDescription(fieldType: MobileFieldType): String {
        return when (fieldType) {
            MobileFieldType.PHONE -> "Tap to call"
            MobileFieldType.EMAIL -> "Tap to email"
            MobileFieldType.WEBSITE -> "Tap to open"
            MobileFieldType.ADDRESS -> "Tap for directions"
            MobileFieldType.SOCIAL -> "Tap to view profile"
            MobileFieldType.CUSTOM -> "Tap to copy"
        }
    }
}
