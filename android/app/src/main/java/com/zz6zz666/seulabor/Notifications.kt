package com.zz6zz666.seulabor

import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicInteger

@SuppressLint("MissingPermission")
object Notifications {
    const val CHANNEL_MONITOR = "monitor"
    const val CHANNEL_ALERTS = "alerts"
    const val CHANNEL_RECOVERY = "recovery"
    const val ID_ONGOING = 1001

    private val nextId = AtomicInteger(2000)

    fun createChannels(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_MONITOR,
                "监控常驻",
                NotificationManager.IMPORTANCE_LOW
            ).apply { setShowBadge(false) }
        )
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ALERTS,
                "课程提醒",
                NotificationManager.IMPORTANCE_HIGH
            )
        )
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_RECOVERY,
                "保活恢复",
                NotificationManager.IMPORTANCE_DEFAULT
            )
        )
    }

    fun ongoing(context: Context, text: String): Notification {
        val open = PendingIntent.getActivity(
            context,
            0,
            Intent(context, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val fetch = PendingIntent.getForegroundService(
            context,
            1,
            Intent(context, MonitorService::class.java).setAction(MonitorService.ACTION_FETCH_NOW),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val rebuild = PendingIntent.getBroadcast(
            context,
            3,
            Intent(context, NotificationDismissReceiver::class.java)
                .setAction(MonitorService.ACTION_REFRESH_NOTIFICATION),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        return NotificationCompat.Builder(context, CHANNEL_MONITOR)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle("课程监控运行中")
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setOngoing(true)
            .setAutoCancel(false)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setShowWhen(false)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .setContentIntent(open)
            .setDeleteIntent(rebuild)
            .addAction(0, "立即刷新", fetch)
            .build()
    }

    fun monitorNotificationEnabled(context: Context): Boolean {
        val manager = context.getSystemService(NotificationManager::class.java)
        val channel = manager.getNotificationChannel(CHANNEL_MONITOR)
        return NotificationManagerCompat.from(context).areNotificationsEnabled() &&
            channel != null &&
            channel.importance != NotificationManager.IMPORTANCE_NONE
    }

    fun alert(context: Context, eventJson: String) {
        runCatching {
            val event = JSONObject(eventJson)
            val open = PendingIntent.getActivity(
                context,
                nextId.get(),
                Intent(context, MainActivity::class.java),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )
            val title = event.optString("title", "课程提醒")
            val body = event.optString("body", "")
            val notification = NotificationCompat.Builder(context, CHANNEL_ALERTS)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle(title)
                .setContentText(body)
                .setStyle(NotificationCompat.BigTextStyle().bigText(body))
                .setAutoCancel(true)
                .setContentIntent(open)
                .build()
            NotificationManagerCompat.from(context).notify(nextId.incrementAndGet(), notification)
        }
    }

    fun recovery(context: Context) {
        runCatching {
            val open = PendingIntent.getActivity(
                context,
                2,
                Intent(context, MainActivity::class.java),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )
            val notification = NotificationCompat.Builder(context, CHANNEL_RECOVERY)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle("监控已停止")
                .setContentText("点击打开应用恢复常驻")
                .setOnlyAlertOnce(true)
                .setAutoCancel(true)
                .setContentIntent(open)
                .build()
            NotificationManagerCompat.from(context).notify(3001, notification)
        }
    }

    fun ongoingText(tickJson: String): String {
        return runCatching {
            val root = JSONObject(tickJson)
            if (root.has("error")) {
                return "运行错误：" + root.optString("error")
            }
            val status = root.optJSONObject("status") ?: JSONObject()
            val message = status.optString("lastMessage", "等待首次抓取")
            val next = if (status.isNull("nextRun")) "" else status.optString("nextRun", "")
            val valid = status.optInt("currentValid", 0)
            val pushed = status.optInt("pushedToday", 0)
            buildString {
                append(message)
                append(" · 可选 ").append(valid).append(" 门")
                if (pushed > 0) append(" · 今日 ").append(pushed)
                if (next.isNotEmpty() && next != "null") append(" · 下次 ").append(next)
            }
        }.getOrDefault("监控运行中")
    }
}
