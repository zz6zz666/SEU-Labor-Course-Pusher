package com.zz6zz666.seulabor

import android.accessibilityservice.AccessibilityService
import android.content.ComponentName
import android.content.Context
import android.provider.Settings
import android.view.accessibility.AccessibilityEvent

/**
 * Optional process keeper. It never reads window content; the system binding is
 * the whole point. Enable it only if the user accepts the extra permission.
 */
class KeepAliveAccessibilityService : AccessibilityService() {
    override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit

    override fun onInterrupt() = Unit

    override fun onServiceConnected() {
        MonitorService.ensureRunning(this)
    }

    companion object {
        fun isEnabled(context: Context): Boolean {
            val expected = ComponentName(context, KeepAliveAccessibilityService::class.java)
            val enabled = Settings.Secure.getString(
                context.contentResolver,
                Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES
            ) ?: return false
            return enabled.split(':').any { ComponentName.unflattenFromString(it) == expected }
        }
    }
}
