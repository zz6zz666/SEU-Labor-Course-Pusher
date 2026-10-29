package com.zz6zz666.seulabor

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest

/**
 * Binds this process to a cellular network while the option is enabled. The
 * core creates its HTTP sockets in this process, so its traffic bypasses Wi-Fi
 * instead of only being reported as "mobile".
 */
object MobileData {
    private val lock = Any()
    private var callback: ConnectivityManager.NetworkCallback? = null
    private var bound: Network? = null

    fun sync(context: Context) {
        if (Prefs.isForceMobileData(context)) {
            enable(context.applicationContext)
        } else {
            disable(context.applicationContext)
        }
    }

    fun setEnabled(context: Context, enabled: Boolean) {
        val app = context.applicationContext
        Prefs.setForceMobileData(app, enabled)
        if (enabled) enable(app) else disable(app)
    }

    private fun enable(context: Context) {
        synchronized(lock) {
            if (callback != null) return
            val manager = context.getSystemService(ConnectivityManager::class.java)
            val request = NetworkRequest.Builder()
                .addTransportType(NetworkCapabilities.TRANSPORT_CELLULAR)
                .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                .build()
            val networkCallback = object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) {
                    synchronized(lock) {
                        if (callback !== this) return
                        if (runCatching { manager.bindProcessToNetwork(network) }
                                .getOrDefault(false)
                        ) {
                            bound = network
                        }
                    }
                }

                override fun onLost(network: Network) {
                    synchronized(lock) {
                        if (callback !== this || bound != network) return
                        runCatching { manager.bindProcessToNetwork(null) }
                        bound = null
                    }
                }

                override fun onUnavailable() {
                    synchronized(lock) {
                        if (callback === this) {
                            callback = null
                            bound = null
                        }
                    }
                }
            }
            callback = networkCallback
            val requested = runCatching {
                manager.requestNetwork(request, networkCallback)
                true
            }.getOrDefault(false)
            if (!requested) {
                callback = null
                bound = null
            }
        }
    }

    private fun disable(context: Context) {
        synchronized(lock) {
            val manager = context.getSystemService(ConnectivityManager::class.java)
            val previous = callback
            callback = null
            bound = null
            runCatching { manager.bindProcessToNetwork(null) }
            if (previous != null) {
                runCatching { manager.unregisterNetworkCallback(previous) }
            }
        }
    }
}
