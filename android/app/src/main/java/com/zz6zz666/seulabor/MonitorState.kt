package com.zz6zz666.seulabor

import android.os.Handler
import android.os.Looper
import java.util.concurrent.CopyOnWriteArrayList

/** Process-wide snapshot of the last tick, so the Activity is only a renderer. */
object MonitorState {
    @Volatile
    private var payload: String = "{}"

    private val listeners = CopyOnWriteArrayList<(String) -> Unit>()
    private val main = Handler(Looper.getMainLooper())

    fun publish(value: String) {
        payload = value
        main.post { listeners.forEach { it(value) } }
    }

    fun current(): String = payload

    fun addListener(listener: (String) -> Unit) {
        listeners.add(listener)
        main.post { listener(payload) }
    }

    fun removeListener(listener: (String) -> Unit) {
        listeners.remove(listener)
    }
}
