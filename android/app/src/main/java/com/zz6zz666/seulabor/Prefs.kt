package com.zz6zz666.seulabor

import android.content.Context

object Prefs {
    private const val NAME = "seu_labor"
    private const val KEY_MONITORING = "monitoringEnabled"
    private const val KEY_AUTO_START = "autoStart"
    private const val KEY_FORCE_MOBILE_DATA = "forceMobileData"
    private const val KEY_THEME = "themeMode"
    private const val KEY_LAST_TICK = "lastTickAt"

    const val THEME_SYSTEM = "system"
    const val THEME_LIGHT = "light"
    const val THEME_DARK = "dark"

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

    fun isForceMobileData(context: Context): Boolean =
        prefs(context).getBoolean(KEY_FORCE_MOBILE_DATA, false)

    fun setForceMobileData(context: Context, value: Boolean) {
        prefs(context).edit().putBoolean(KEY_FORCE_MOBILE_DATA, value).apply()
    }

    fun themeMode(context: Context): String =
        prefs(context).getString(KEY_THEME, THEME_SYSTEM) ?: THEME_SYSTEM

    fun setThemeMode(context: Context, value: String) {
        prefs(context).edit().putString(KEY_THEME, value).apply()
    }

    fun lastTickAt(context: Context): Long =
        prefs(context).getLong(KEY_LAST_TICK, 0L)

    fun setLastTickAt(context: Context, value: Long) {
        prefs(context).edit().putLong(KEY_LAST_TICK, value).apply()
    }
}
