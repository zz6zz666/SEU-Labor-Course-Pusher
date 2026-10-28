package com.zz6zz666.seulabor

import android.app.Activity
import android.content.Intent

/** Child screens always return to the status page, even if the task was pruned. */
object Nav {
    fun backToMain(activity: Activity) {
        activity.startActivity(
            Intent(activity, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        )
        activity.finish()
    }
}
