package com.zz6zz666.seulabor

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Typeface
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.os.SystemClock
import android.provider.Settings
import android.util.TypedValue
import android.view.Gravity
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Switch
import android.widget.TextView
import android.widget.Toast
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import org.json.JSONObject

/** Process-wide debounce so the battery dialog cannot re-open itself on return. */
object KeepAliveGate {
    @Volatile
    var lastBatteryPromptAt: Long = 0L
}

class MainActivity : Activity() {
    private lateinit var statusTitle: TextView
    private lateinit var statusDetail: TextView
    private lateinit var keepAliveText: TextView
    private lateinit var autoStartSwitch: Switch
    private lateinit var autoSelectSwitch: Switch

    private var navigating = false
    private val listener: (String) -> Unit = { render(it) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        runCatching { CoreBridge.nativeInit(filesDir.absolutePath) }
        setContentView(buildUi())
        ensureNotificationPermission()
        MonitorService.ensureRunning(this)
        MonitorState.addListener(listener)
        refresh()
    }

    override fun onResume() {
        super.onResume()
        navigating = false
        refresh()
        refreshSwitches()
        ensureBatteryExemption()
        MonitorService.ensureRunning(this)
        MonitorService.refreshNotification(this)
    }

    override fun onStop() {
        super.onStop()
        // The polling loop lives in the service; the page itself has nothing to
        // keep alive once the user leaves it.
        if (!navigating && !isChangingConfigurations) finish()
    }

    override fun onDestroy() {
        MonitorState.removeListener(listener)
        super.onDestroy()
    }

    override fun onNewIntent(intent: Intent?) {
        super.onNewIntent(intent)
        refresh()
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        refresh()
    }

    private fun buildUi(): ScrollView {
        val root = ScrollView(this)
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(24), dp(20), dp(32))
            setBackgroundColor(Color.parseColor("#F7F8FA"))
        }
        root.addView(column)
        ViewCompat.setOnApplyWindowInsetsListener(root) { view, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            view.setPadding(0, bars.top, 0, bars.bottom)
            insets
        }
        ViewCompat.requestApplyInsets(root)

        column.addView(TextView(this).apply {
            text = "课程监控"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 26f)
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.parseColor("#141821"))
        })

        val card = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(18), dp(16), dp(18), dp(16))
            background = getDrawable(R.drawable.card_bg)
        }
        statusTitle = TextView(this).apply {
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 17f)
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.parseColor("#141821"))
        }
        statusDetail = TextView(this).apply {
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
            setTextColor(Color.parseColor("#5B6270"))
            setLineSpacing(dp(2).toFloat(), 1f)
        }
        card.addView(statusTitle)
        card.addView(statusDetail, fullWidth().apply { topMargin = dp(6) })
        column.addView(card, fullWidth().apply { topMargin = dp(18) })

        column.addView(actionButton("打开选课页") {
            openScreen(CourseActivity::class.java)
        })
        column.addView(actionButton("重新登录") {
            openScreen(LoginActivity::class.java)
        })
        column.addView(actionButton("立即抓取一次") {
            startService(
                Intent(this, MonitorService::class.java)
                    .setAction(MonitorService.ACTION_FETCH_NOW)
            )
        })

        val secondary = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        secondary.addView(
            smallButton("设置") {
                openScreen(SettingsActivity::class.java)
            },
            LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
        )
        secondary.addView(
            smallButton("日志") {
                openScreen(LogsActivity::class.java)
            },
            LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f).apply {
                marginStart = dp(10)
            }
        )
        column.addView(secondary, fullWidth().apply { topMargin = dp(10) })

        val autoStartSwitch = Switch(this).apply {
            text = "开机自启"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
            isChecked = Prefs.isAutoStart(this@MainActivity)
            setOnCheckedChangeListener { _, value ->
                Prefs.setAutoStart(this@MainActivity, value)
            }
        }
        this.autoStartSwitch = autoStartSwitch
        column.addView(autoStartSwitch, fullWidth().apply { topMargin = dp(16) })

        val autoSelectSwitch = Switch(this).apply {
            text = "自动选课"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
            val cfg = runCatching { JSONObject(CoreBridge.nativeGetConfig()) }.getOrNull()
            isChecked = cfg?.optJSONObject("behavior")?.optBoolean("autoSelect", false) ?: false
            setOnCheckedChangeListener { _, value -> setAutoSelect(value) }
        }
        this.autoSelectSwitch = autoSelectSwitch
        column.addView(autoSelectSwitch, fullWidth().apply { topMargin = dp(8) })

        val keepCard = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(18), dp(16), dp(18), dp(16))
            background = getDrawable(R.drawable.card_bg)
        }
        keepCard.addView(TextView(this).apply {
            text = "保活状态"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.parseColor("#141821"))
        })
        keepAliveText = TextView(this).apply {
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
            setTextColor(Color.parseColor("#5B6270"))
            setLineSpacing(dp(3).toFloat(), 1f)
        }
        keepCard.addView(keepAliveText, fullWidth().apply { topMargin = dp(6) })
        keepCard.addView(smallButton("保活检查") { showKeepAliveDialog() }, fullWidth().apply {
            topMargin = dp(10)
        })
        column.addView(keepCard, fullWidth().apply { topMargin = dp(18) })

        val version = runCatching {
            packageManager.getPackageInfo(packageName, 0).versionName
        }.getOrNull()
        column.addView(TextView(this).apply {
            text = "版本 $version"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTextColor(Color.parseColor("#9AA0AC"))
            gravity = Gravity.CENTER
        }, fullWidth().apply { topMargin = dp(24) })

        return root
    }

    private fun actionButton(text: String, onClick: () -> Unit): Button =
        Button(this).apply {
            this.text = text
            isAllCaps = false
            setOnClickListener { onClick() }
            layoutParams = fullWidth().apply { topMargin = dp(10) }
        }

    private fun openScreen(target: Class<*>) {
        navigating = true
        startActivity(Intent(this, target))
    }

    private fun smallButton(text: String, onClick: () -> Unit): Button =
        Button(this).apply {
            this.text = text
            isAllCaps = false
            setOnClickListener { onClick() }
        }

    private fun fullWidth(): LinearLayout.LayoutParams =
        LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT,
            LinearLayout.LayoutParams.WRAP_CONTENT
        )

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    private fun refresh() {
        val json = runCatching { CoreBridge.nativeStatus() }.getOrDefault("{}")
        render(json)
    }

    private fun render(json: String) {
        runCatching {
            val root = JSONObject(json)
            if (root.has("error")) {
                statusDetail.text = root.optString("error")
                return
            }
            val status = root.optJSONObject("status") ?: JSONObject()
            val login = when (root.optString("authState", "unknown")) {
                "valid" -> "登录正常"
                "expired" -> "登录失效"
                else -> "尚未登录"
            }
            statusTitle.text = "$login · 可选 ${status.optInt("currentValid")} 门 · 今日 ${status.optInt("pushedToday")} 门"
            val message = status.optString("lastMessage", "等待首次抓取")
            val next = if (status.isNull("nextRun")) "" else status.optString("nextRun", "")
            statusDetail.text = if (next.isEmpty() || next == "null") {
                message
            } else {
                "$message（下次 $next）"
            }
            keepAliveText.text = keepAliveSummary()
        }
    }

    private fun refreshSwitches() {
        autoStartSwitch.isChecked = Prefs.isAutoStart(this)
        val autoSelect = runCatching {
            JSONObject(CoreBridge.nativeGetConfig())
                .optJSONObject("behavior")
                ?.optBoolean("autoSelect", false)
        }.getOrNull() ?: false
        autoSelectSwitch.isChecked = autoSelect
    }

    private fun keepAliveSummary(): String {
        val notifications = Notifications.monitorNotificationEnabled(this)
        val battery = getSystemService(PowerManager::class.java)
            .isIgnoringBatteryOptimizations(packageName)
        val service = MonitorService.isRunning
        val accessibility = KeepAliveAccessibilityService.isEnabled(this)
        return "通知 ${mark(notifications)}   电池 ${mark(battery)}   " +
            "服务 ${mark(service)}   无障碍 ${mark(accessibility)}"
    }

    private fun mark(ok: Boolean): String = if (ok) "✓" else "✗"

    private fun ensureNotificationPermission() {
        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 101)
        }
    }

    private fun ensureBatteryExemption() {
        val manager = getSystemService(PowerManager::class.java)
        if (manager.isIgnoringBatteryOptimizations(packageName)) return

        val now = SystemClock.elapsedRealtime()
        if (now - KeepAliveGate.lastBatteryPromptAt < 60_000L) return
        KeepAliveGate.lastBatteryPromptAt = now
        requestBatteryExemption()
    }

    private fun requestBatteryExemption() {
        navigating = true
        runCatching {
            startActivity(
                Intent(
                    Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
                    Uri.parse("package:$packageName")
                )
            )
        }.onFailure {
            OemSettings.openAppDetails(this)
        }
    }

    private fun showKeepAliveDialog() {
        val items = arrayOf(
            "通知权限",
            "通知渠道（静音常驻）",
            "电池不受限制",
            "厂商自启动 / 关联启动",
            "无障碍增强（可选）"
        )
        AlertDialog.Builder(this)
            .setTitle("保活检查")
            .setItems(items) { _, which ->
                when (which) {
                    0 -> ensureNotificationPermission()
                    1 -> {
                        navigating = true
                        startActivity(
                            Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                                .putExtra(Settings.EXTRA_APP_PACKAGE, packageName)
                        )
                    }
                    2 -> requestBatteryExemption()
                    3 -> {
                        navigating = true
                        OemSettings.openAutoStart(this)
                    }
                    4 -> {
                        navigating = true
                        toast("若开关不可用，请在应用信息右上角允许受限设置")
                        startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
                    }
                }
            }
            .setNegativeButton("关闭", null)
            .show()
    }

    private fun setAutoSelect(value: Boolean) {
        runCatching {
            val cfg = JSONObject(CoreBridge.nativeGetConfig())
            val behavior = cfg.optJSONObject("behavior")
                ?: JSONObject().also { cfg.put("behavior", it) }
            behavior.put("autoSelect", value)
            val result = JSONObject(CoreBridge.nativeSetConfig(cfg.toString()))
            if (!result.optBoolean("ok", false)) {
                toast(result.optString("error", "保存失败"))
            }
        }.onFailure { toast(it.message ?: "保存失败") }
    }

    private fun toast(text: String) {
        Toast.makeText(this, text, Toast.LENGTH_SHORT).show()
    }
}
