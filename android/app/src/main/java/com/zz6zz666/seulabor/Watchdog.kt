package com.zz6zz666.seulabor

import android.content.Context
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ListenableWorker.Result
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit

object Watchdog {
    fun schedule(context: Context) {
        val request = PeriodicWorkRequestBuilder<WatchdogWorker>(15, TimeUnit.MINUTES).build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(
            "seu-watchdog",
            ExistingPeriodicWorkPolicy.KEEP,
            request
        )
    }
}

class WatchdogWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
    override fun doWork(): Result {
        val stale = System.currentTimeMillis() - Prefs.lastTickAt(applicationContext) > 20 * 60_000L
        if (!MonitorService.isRunning || stale) {
            Notifications.recovery(applicationContext)
            MonitorService.ensureRunning(applicationContext)
        }
        return Result.success()
    }
}
