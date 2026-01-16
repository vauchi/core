package com.webbook.worker

import android.content.Context
import android.util.Log
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import com.webbook.data.WebBookRepository

class SyncWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    companion object {
        const val TAG = "SyncWorker"
        const val WORK_NAME = "webbook_periodic_sync"
    }

    override suspend fun doWork(): Result {
        Log.d(TAG, "Starting background sync")

        return try {
            val repository = WebBookRepository(applicationContext)

            // Only sync if identity exists
            if (!repository.hasIdentity()) {
                Log.d(TAG, "No identity found, skipping sync")
                return Result.success()
            }

            val result = repository.sync()
            Log.d(TAG, "Sync complete: ${result.contactsAdded} contacts added, ${result.cardsUpdated} cards updated")
            Result.success()
        } catch (e: Exception) {
            Log.e(TAG, "Sync failed: ${e.message}", e)
            if (runAttemptCount < 3) {
                Log.d(TAG, "Retrying sync (attempt ${runAttemptCount + 1})")
                Result.retry()
            } else {
                Log.e(TAG, "Max retry attempts reached, giving up")
                Result.failure()
            }
        }
    }
}
