package com.vauchi.util

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Clipboard utilities with automatic clearing for sensitive data.
 */
object ClipboardUtils {

    private const val CLEAR_DELAY_MS = 30_000L // 30 seconds

    /**
     * Copy text to clipboard with automatic clearing after 30 seconds.
     *
     * @param context Android context
     * @param scope Coroutine scope for the auto-clear timer
     * @param text The text to copy
     * @param label Label for the clipboard data
     */
    fun copyWithAutoClear(
        context: Context,
        scope: CoroutineScope,
        text: String,
        label: String = "Vauchi"
    ) {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = ClipData.newPlainText(label, text)
        clipboard.setPrimaryClip(clip)

        // Schedule auto-clear after 30 seconds
        scope.launch {
            delay(CLEAR_DELAY_MS)
            // Only clear if clipboard still contains what we copied
            val currentClip = clipboard.primaryClip
            if (currentClip != null && currentClip.itemCount > 0) {
                val currentText = currentClip.getItemAt(0).text?.toString()
                if (currentText == text) {
                    // Clear by setting empty clip
                    clipboard.setPrimaryClip(ClipData.newPlainText("", ""))
                }
            }
        }
    }

    /**
     * Copy text to clipboard without auto-clear (for non-sensitive data).
     */
    fun copy(context: Context, text: String, label: String = "Vauchi") {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = ClipData.newPlainText(label, text)
        clipboard.setPrimaryClip(clip)
    }
}
