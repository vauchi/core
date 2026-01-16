package com.webbook

import android.app.Application
import android.util.Log
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import com.webbook.worker.SyncWorker
import java.util.concurrent.TimeUnit

class WebBookApp : Application() {

    override fun onCreate() {
        super.onCreate()
        instance = this
        schedulePeriodicSync()
    }

    private fun schedulePeriodicSync() {
        Log.d(TAG, "Scheduling periodic sync")

        val constraints = Constraints.Builder()
            .setRequiredNetworkType(NetworkType.CONNECTED)
            .build()

        val syncRequest = PeriodicWorkRequestBuilder<SyncWorker>(
            repeatInterval = 15,
            repeatIntervalTimeUnit = TimeUnit.MINUTES
        )
            .setConstraints(constraints)
            .build()

        WorkManager.getInstance(this).enqueueUniquePeriodicWork(
            SyncWorker.WORK_NAME,
            ExistingPeriodicWorkPolicy.KEEP,
            syncRequest
        )

        Log.d(TAG, "Periodic sync scheduled (every 15 minutes)")
    }

    companion object {
        private const val TAG = "WebBookApp"

        lateinit var instance: WebBookApp
            private set
    }
}
