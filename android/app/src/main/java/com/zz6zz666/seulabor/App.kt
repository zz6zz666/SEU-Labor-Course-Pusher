package com.zz6zz666.seulabor

import android.app.Application
import androidx.appcompat.app.AppCompatDelegate

class App : Application() {
    override fun onCreate() {
        super.onCreate()
        AppCompatDelegate.setDefaultNightMode(nightMode(Prefs.themeMode(this)))
        MobileData.sync(this)
        Notifications.createChannels(this)
        runCatching { CoreBridge.nativeInit(filesDir.absolutePath) }
        Watchdog.schedule(this)
    }

    companion object {
        fun nightMode(mode: String): Int = when (mode) {
            Prefs.THEME_LIGHT -> AppCompatDelegate.MODE_NIGHT_NO
            Prefs.THEME_DARK -> AppCompatDelegate.MODE_NIGHT_YES
            else -> AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM
        }
    }
}
