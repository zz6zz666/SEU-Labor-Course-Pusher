package com.zz6zz666.seulabor

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.core.content.ContextCompat

class NotificationDismissReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        if (intent?.action != MonitorService.ACTION_REFRESH_NOTIFICATION) return
        runCatching {
            ContextCompat.startForegroundService(
                context,
                Intent(context, MonitorService::class.java)
                    .setAction(MonitorService.ACTION_REFRESH_NOTIFICATION)
            )
        }
    }
}
