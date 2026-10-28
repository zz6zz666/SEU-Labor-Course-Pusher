package com.zz6zz666.seulabor

import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.graphics.Typeface
import android.os.Bundle
import android.util.TypedValue
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import java.io.File

class LogsActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val dir = File(filesDir, "logs")
        val latest = dir.listFiles()
            ?.filter { it.isFile }
            ?.maxByOrNull { it.lastModified() }
        val text = latest?.let { runCatching { it.readText() }.getOrNull() }
            ?.takeLast(200_000)
            ?: "暂无日志"

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.WHITE)
            setPadding(dp(16), dp(16), dp(16), dp(16))
        }
        root.addView(TextView(this).apply {
            this.text = latest?.absolutePath ?: "日志目录：${dir.absolutePath}"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 11f)
            setTextColor(Color.parseColor("#9AA0AC"))
        })
        root.addView(Button(this).apply {
            this.text = "分享日志"
            isAllCaps = false
            setOnClickListener {
                val share = Intent(Intent.ACTION_SEND).apply {
                    type = "text/plain"
                    putExtra(Intent.EXTRA_TEXT, text)
                }
                startActivity(Intent.createChooser(share, "分享日志"))
            }
        }, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.WRAP_CONTENT,
            LinearLayout.LayoutParams.WRAP_CONTENT
        ).apply { topMargin = dp(8) })

        val scroll = ScrollView(this)
        scroll.addView(TextView(this).apply {
            this.text = text
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 11f)
            typeface = Typeface.MONOSPACE
            setTextColor(Color.parseColor("#303540"))
        })
        root.addView(scroll, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f
        ))
        setContentView(root)
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        Nav.backToMain(this)
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()
}
