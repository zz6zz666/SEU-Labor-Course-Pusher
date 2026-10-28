package com.zz6zz666.seulabor

import android.content.Context

object Prefs {
    private const val NAME = "seu_labor"
    private const val KEY_MONITORING = "monitoringEnabled"
    private const val KEY_AUTO_START = "autoStart"
    private const val KEY_LAST_TICK = "lastTickAt"

    private fun prefs(context: Context) =
        context.getSharedPreferences(NAME, Context.MODE_PRIVATE)

    fun isMonitoringEnabled(context: Context): Boolean =
        prefs(context).getBoolean(KEY_MONITORING, true)

    fun setMonitoringEnabled(context: Context, value: Boolean) {
        prefs(context).edit().putBoolean(KEY_MONITORING, value).apply()
    }

    fun isAutoStart(context: Context): Boolean =
        prefs(context).getBoolean(KEY_AUTO_START, true)

    fun setAutoStart(context: Context, value: Boolean) {
        prefs(context).edit().putBoolean(KEY_AUTO_START, value).apply()
    }

    fun lastTickAt(context: Context): Long =
        prefs(context).getLong(KEY_LAST_TICK, 0L)

    fun setLastTickAt(context: Context, value: Long) {
        prefs(context).edit().putLong(KEY_LAST_TICK, value).apply()
    }
}
