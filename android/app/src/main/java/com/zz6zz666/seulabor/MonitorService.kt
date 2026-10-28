package com.zz6zz666.seulabor

import android.annotation.SuppressLint
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import org.json.JSONObject

@SuppressLint("WakelockTimeout")
class MonitorService : Service() {
    companion object {
        const val ACTION_FETCH_NOW = "com.zz6zz666.seulabor.FETCH_NOW"
        const val ACTION_REFRESH_NOTIFICATION = "com.zz6zz666.seulabor.REFRESH_NOTIFICATION"

        @Volatile
        var isRunning: Boolean = false

        fun ensureRunning(context: Context) {
            if (isRunning) return
            runCatching {
                ContextCompat.startForegroundService(
                    context,
                    Intent(context, MonitorService::class.java)
                )
            }
        }

        fun refreshNotification(context: Context) {
            runCatching {
                ContextCompat.startForegroundService(
                    context,
                    Intent(context, MonitorService::class.java)
                        .setAction(ACTION_REFRESH_NOTIFICATION)
                )
            }
        }
    }

    @Volatile
    private var running = false

    private var worker: Thread? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var lastAutoLoginAt = 0L

    @Volatile
    private var lastTick = ""

    override fun onCreate() {
        super.onCreate()
        startForegroundInternal("正在启动…")
        // A foreground service keeps the process alive but does not by itself
        // keep the CPU awake; the polling loop needs this to survive screen-off.
        wakeLock = getSystemService(PowerManager::class.java)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "seu-labor:monitor")
            .apply {
                setReferenceCounted(false)
                acquire()
            }
        isRunning = true
        running = true
        worker = Thread({ runLoop() }, "seu-monitor").apply {
            isDaemon = true
            start()
        }
    }

    private fun startForegroundInternal(text: String) {
        val notification = Notifications.ongoing(this, text)
        if (Build.VERSION.SDK_INT >= 34) {
            ServiceCompat.startForeground(
                this,
                Notifications.ID_ONGOING,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
            )
        } else {
            startForeground(Notifications.ID_ONGOING, notification)
        }
    }

    private fun updateForeground(tickJson: String) {
        lastTick = tickJson
        startForegroundInternal(Notifications.ongoingText(tickJson))
    }

    private fun reassertForeground() {
        val tick = lastTick
        startForegroundInternal(
            if (tick.isEmpty()) "监控运行中" else Notifications.ongoingText(tick)
        )
    }

    private fun runLoop() {
        while (running) {
            val tick = runCatching { CoreBridge.nativeTick() }
                .getOrElse { errorJson(it) }

            Prefs.setLastTickAt(this, System.currentTimeMillis())
            MonitorState.publish(tick)
            updateForeground(tick)
            val relogged = handleEvents(tick)

            val delay = if (relogged) {
                5_000L
            } else {
                runCatching {
                    JSONObject(tick).optLong("nextDelayMs", 180_000L)
                }.getOrDefault(180_000L)
            }

            try {
                sleepWithHeartbeat(delay.coerceIn(15_000L, 30 * 60_000L))
            } catch (_: InterruptedException) {
                return
            }
        }
    }

    private fun sleepWithHeartbeat(total: Long) {
        val deadline = System.currentTimeMillis() + total
        while (running) {
            val left = deadline - System.currentTimeMillis()
            if (left <= 0) return
            // Keep the watchdog heartbeat fresh even during a long backoff.
            Prefs.setLastTickAt(this, System.currentTimeMillis())
            try {
                Thread.sleep(minOf(left, 60_000L))
            } catch (_: InterruptedException) {
                return
            }
            // Android 13+ lets the user swipe away a foreground notification;
            // only startForeground() can put it back.
            reassertForeground()
        }
    }

    private fun handleEvents(tickJson: String): Boolean {
        return runCatching {
            val root = JSONObject(tickJson)
            val events = root.optJSONArray("events")
            if (events != null) {
                for (i in 0 until events.length()) {
                    Notifications.alert(this, events.getJSONObject(i).toString())
                }
            }
            if (root.optString("authState") == "expired") {
                trySilentLogin()
            } else {
                false
            }
        }.getOrDefault(false)
    }

    private fun trySilentLogin(): Boolean {
        val now = System.currentTimeMillis()
        if (now - lastAutoLoginAt < 5 * 60_000L) return false

        val config = runCatching { JSONObject(CoreBridge.nativeGetConfig()) }
            .getOrElse { return false }
        val behavior = config.optJSONObject("behavior") ?: return false
        if (!behavior.optBoolean("autoLogin", true)) return false
        val credentials = config.optJSONObject("credentials") ?: return false
        val user = credentials.optString("username", "")
        val pass = credentials.optString("password", "")
        if (user.isEmpty() || pass.isEmpty()) return false

        lastAutoLoginAt = now
        val result = runCatching { JSONObject(CoreBridge.nativeAutoLogin(user, pass)) }
            .getOrNull() ?: return false
        return result.optString("outcome") == "success"
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_FETCH_NOW -> Thread({
                val tick = runCatching { CoreBridge.nativeTick() }.getOrNull() ?: return@Thread
                MonitorState.publish(tick)
                updateForeground(tick)
                handleEvents(tick)
            }, "seu-fetch-now").start()

            ACTION_REFRESH_NOTIFICATION -> reassertForeground()
        }
        return START_STICKY
    }

    override fun onDestroy() {
        running = false
        isRunning = false
        worker?.interrupt()
        worker = null
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun errorJson(error: Throwable): String =
        """{"error":${JSONObject.quote(error.message ?: error.javaClass.simpleName)}}"""
}
