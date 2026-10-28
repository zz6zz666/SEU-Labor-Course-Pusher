package com.zz6zz666.seulabor

import android.app.Application

class App : Application() {
    override fun onCreate() {
        super.onCreate()
        Notifications.createChannels(this)
        runCatching { CoreBridge.nativeInit(filesDir.absolutePath) }
        Watchdog.schedule(this)
    }
}
