package com.zz6zz666.seulabor

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.core.content.ContextCompat

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        if (!Prefs.isMonitoringEnabled(context)) return
        if (!Prefs.isAutoStart(context)) return
        runCatching {
            ContextCompat.startForegroundService(
                context,
                Intent(context, MonitorService::class.java)
            )
        }.onFailure {
            // Android 15+ can refuse a boot-time start. The watchdog notification
            // brings the user back to the UI, where a start is always allowed.
            Notifications.recovery(context)
        }
    }
}
