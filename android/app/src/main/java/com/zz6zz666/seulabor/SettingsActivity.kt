package com.zz6zz666.seulabor

import android.content.Intent
import android.graphics.Typeface
import android.os.Bundle
import android.text.InputType
import android.text.method.LinkMovementMethod
import android.text.util.Linkify
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioGroup
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.app.AppCompatDelegate
import androidx.appcompat.widget.AppCompatButton
import androidx.appcompat.widget.AppCompatCheckBox
import androidx.appcompat.widget.AppCompatEditText
import androidx.appcompat.widget.AppCompatRadioButton
import androidx.appcompat.widget.SwitchCompat
import org.json.JSONArray
import org.json.JSONObject

class SettingsActivity : AppCompatActivity() {
    companion object {
        private val LABOR_TYPES = listOf("生活劳动", "服务劳动", "生产劳动")
    }

    private lateinit var config: JSONObject
    private lateinit var username: EditText
    private lateinit var password: EditText
    private lateinit var pushplusToken: EditText
    private lateinit var locations: EditText
    private val categoryChecks = linkedMapOf<String, CheckBox>()
    private lateinit var autoLoginSwitch: SwitchCompat
    private lateinit var autoSelectSwitch: SwitchCompat
    private lateinit var saveButton: Button
    private var originalUsername = ""
    private var originalPassword = ""

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        config = runCatching { JSONObject(CoreBridge.nativeGetConfig()) }.getOrElse { JSONObject() }
        setContentView(buildUi())
    }

    private fun buildUi(): ScrollView {
        val root = ScrollView(this)
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(24), dp(20), dp(32))
            setBackgroundColor(getColor(R.color.app_background))
        }
        root.addView(column)

        column.addView(TextView(this).apply {
            text = "设置"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 26f)
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(getColor(R.color.app_text_primary))
        })

        val credentials = config.optJSONObject("credentials") ?: JSONObject()
        originalUsername = credentials.optString("username", "")
        originalPassword = credentials.optString("password", "")
        username = addField(
            column,
            "一卡通号",
            originalUsername
        )
        password = addField(
            column,
            "密码",
            originalPassword,
            password = true
        )

        val push = config.optJSONObject("push")
        val pushplus = push?.optJSONObject("pushplus")
        pushplusToken = addField(
            column,
            "PushPlus Token",
            pushplus?.optString("token", "") ?: ""
        )
        pushplusToken.hint = "可选，留空即只用桌面通知"
        val pushHint = TextView(this).apply {
            text = "申请地址：www.pushplus.plus　微信扫码登录后，注册并复制 用户Token。\n" +
                "（可能需要向 PushPlus 平台支付 1 元以激活微信推送功能）"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTextColor(getColor(R.color.app_text_tertiary))
            setLineSpacing(dp(2).toFloat(), 1f)
            setLinkTextColor(getColor(R.color.app_accent))
            movementMethod = LinkMovementMethod.getInstance()
        }
        Linkify.addLinks(pushHint, Linkify.WEB_URLS)
        column.addView(pushHint, fullWidth().apply { topMargin = dp(4) })

        val filters = config.optJSONObject("filters")
        locations = addField(
            column,
            "开课地点关键字",
            join(filters?.optJSONArray("locationWhitelist"))
        )
        locations.hint = "例如：无锡、九龙湖、四牌楼、丁家桥（留空则不筛地点）"
        column.addView(TextView(this).apply {
            text = "多个关键字用逗号或空格分隔；开课地点包含任一关键字即命中。"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTextColor(getColor(R.color.app_text_tertiary))
            setLineSpacing(dp(2).toFloat(), 1f)
        }, fullWidth().apply { topMargin = dp(4) })

        val blocked = stringList(filters?.optJSONArray("categoryBlacklist"))
        column.addView(TextView(this).apply {
            text = "劳动类型"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
            setTextColor(getColor(R.color.app_text_secondary))
        }, fullWidth().apply { topMargin = dp(14) })

        val row = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        for (type in LABOR_TYPES) {
            val check = AppCompatCheckBox(this).apply {
                text = type
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
                isChecked = type !in blocked
            }
            row.addView(check, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            categoryChecks[type] = check
        }
        column.addView(row, fullWidth().apply { topMargin = dp(2) })
        column.addView(TextView(this).apply {
            text = "勾选你想接收的类型；全部勾选或全部不勾选均表示不限制。"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTextColor(getColor(R.color.app_text_tertiary))
            setLineSpacing(dp(2).toFloat(), 1f)
        }, fullWidth().apply { topMargin = dp(4) })

        val behavior = config.optJSONObject("behavior") ?: JSONObject()
        autoLoginSwitch = SwitchCompat(this).apply {
            text = "登录失效时自动重登"
            isChecked = behavior.optBoolean("autoLogin", true)
        }
        column.addView(autoLoginSwitch, fullWidth().apply { topMargin = dp(16) })

        autoSelectSwitch = SwitchCompat(this).apply {
            text = "自动选课"
            isChecked = behavior.optBoolean("autoSelect", false)
        }
        column.addView(autoSelectSwitch, fullWidth().apply { topMargin = dp(8) })

        column.addView(TextView(this).apply {
            text = "外观（默认跟随系统）"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
            setTextColor(getColor(R.color.app_text_secondary))
        }, fullWidth().apply { topMargin = dp(16) })

        val themeIds = linkedMapOf<Int, String>()
        val currentTheme = Prefs.themeMode(this)
        val themeGroup = RadioGroup(this).apply {
            orientation = RadioGroup.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        listOf(
            Prefs.THEME_SYSTEM to "跟随系统",
            Prefs.THEME_LIGHT to "浅色",
            Prefs.THEME_DARK to "深色"
        ).forEach { (mode, label) ->
            val radio = AppCompatRadioButton(this).apply {
                id = View.generateViewId()
                text = label
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
            }
            themeIds[radio.id] = mode
            themeGroup.addView(
                radio,
                LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
            )
        }
        themeIds.entries.firstOrNull { it.value == currentTheme }?.let {
            themeGroup.check(it.key)
        }
        themeGroup.setOnCheckedChangeListener { _, id ->
            val mode = themeIds[id] ?: return@setOnCheckedChangeListener
            if (mode == Prefs.themeMode(this)) return@setOnCheckedChangeListener
            Prefs.setThemeMode(this, mode)
            AppCompatDelegate.setDefaultNightMode(App.nightMode(mode))
        }
        column.addView(themeGroup, fullWidth().apply { topMargin = dp(4) })

        saveButton = AppCompatButton(this).apply {
            text = "保存"
            isAllCaps = false
            setOnClickListener { save() }
            layoutParams = fullWidth().apply { topMargin = dp(22) }
        }
        column.addView(saveButton)

        column.addView(TextView(this).apply {
            text = "账号与设置仅保存在本机应用私有目录。"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTextColor(getColor(R.color.app_text_tertiary))
            gravity = Gravity.CENTER
        }, fullWidth().apply { topMargin = dp(16) })

        return root
    }

    private fun addField(
        column: LinearLayout,
        label: String,
        value: String,
        password: Boolean = false
    ): EditText {
        column.addView(TextView(this).apply {
            text = label
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
            setTextColor(getColor(R.color.app_text_secondary))
        }, fullWidth().apply { topMargin = dp(14) })

        val field = AppCompatEditText(this).apply {
            setText(value)
            setSingleLine(true)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
            if (password) {
                inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            }
        }
        column.addView(field, fullWidth().apply { topMargin = dp(6) })
        return field
    }

    private fun save() {
        if (!saveButton.isEnabled) return
        val user = username.text.toString().trim()
        val pass = password.text.toString()
        if (user.isEmpty() || pass.isEmpty()) {
            Toast.makeText(this, "请填写一卡通号和密码", Toast.LENGTH_LONG).show()
            return
        }
        runCatching {
            val credentials = config.optJSONObject("credentials")
                ?: JSONObject().also { config.put("credentials", it) }
            credentials.put("username", user)
            credentials.put("password", pass)

            val filters = config.optJSONObject("filters")
                ?: JSONObject().also { config.put("filters", it) }
            filters.put("locationWhitelist", toArray(locations.text.toString()))

            // The UI is a whitelist of labor types, but the persisted contract is
            // still categoryBlacklist: only the unchecked types are written.
            val chosen = categoryChecks.filterValues { it.isChecked }.keys
            val blocked = JSONArray()
            if (chosen.isNotEmpty() && chosen.size < LABOR_TYPES.size) {
                LABOR_TYPES.filter { it !in chosen }.forEach { blocked.put(it) }
            }
            filters.put("categoryBlacklist", blocked)

            val push = config.optJSONObject("push")
                ?: JSONObject().also { config.put("push", it) }
            val pushplus = push.optJSONObject("pushplus")
                ?: JSONObject().also { push.put("pushplus", it) }
            val token = pushplusToken.text.toString().trim()
            pushplus.put("token", token)
            pushplus.put("enabled", token.isNotEmpty())

            val behavior = config.optJSONObject("behavior")
                ?: JSONObject().also { config.put("behavior", it) }
            behavior.put("autoLogin", autoLoginSwitch.isChecked)
            behavior.put("autoSelect", autoSelectSwitch.isChecked)

            val result = JSONObject(CoreBridge.nativeSetConfig(config.toString()))
            if (!result.optBoolean("ok", false)) {
                Toast.makeText(this, result.optString("error", "保存失败"), Toast.LENGTH_LONG).show()
                return
            }
            if (autoLoginSwitch.isChecked &&
                (user != originalUsername || pass != originalPassword || !sessionValid())
            ) {
                beginAutoLogin(user, pass)
            } else {
                Toast.makeText(this, "已保存", Toast.LENGTH_SHORT).show()
                afterSave()
            }
        }.onFailure {
            Toast.makeText(this, it.message ?: "保存失败", Toast.LENGTH_LONG).show()
        }
    }

    private fun sessionValid(): Boolean {
        val status = runCatching { JSONObject(CoreBridge.nativeStatus()) }.getOrNull()
        return status?.optString("authState") == "valid"
    }

    private fun beginAutoLogin(user: String, pass: String) {
        saveButton.isEnabled = false
        saveButton.text = "正在登录…"
        Toast.makeText(this, "正在尝试自动登录…", Toast.LENGTH_SHORT).show()
        Thread({
            val result = runCatching {
                JSONObject(CoreBridge.nativeAutoLogin(user, pass))
            }.getOrElse {
                JSONObject()
                    .put("outcome", "error")
                    .put("detail", it.message ?: "自动登录失败")
            }
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                saveButton.isEnabled = true
                saveButton.text = "保存"
                when (result.optString("outcome")) {
                    "success" -> {
                        Toast.makeText(this, "登录成功", Toast.LENGTH_SHORT).show()
                        afterSave()
                    }
                    "needManual" -> {
                        Toast.makeText(
                            this,
                            result.optString("detail", "需要验证码或短信验证"),
                            Toast.LENGTH_LONG
                        ).show()
                        startActivity(Intent(this, LoginActivity::class.java))
                        finish()
                    }
                    else -> Toast.makeText(
                        this,
                        result.optString("detail", "自动登录失败"),
                        Toast.LENGTH_LONG
                    ).show()
                }
            }
        }, "settings-login").start()
    }

    private fun afterSave() {
        startService(
            Intent(this, MonitorService::class.java)
                .setAction(MonitorService.ACTION_FETCH_NOW)
        )
        Nav.backToMain(this)
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        Nav.backToMain(this)
    }

    private fun toArray(raw: String): JSONArray {
        val array = JSONArray()
        raw.split(Regex("[,，、;；\\s\\u3000]+"))
            .map(String::trim)
            .filter { it.isNotEmpty() }
            .forEach { array.put(it) }
        return array
    }

    private fun join(array: JSONArray?): String {
        if (array == null) return ""
        return (0 until array.length()).map { array.optString(it) }.joinToString("，")
    }

    private fun stringList(array: JSONArray?): List<String> {
        if (array == null) return emptyList()
        return (0 until array.length()).map { array.optString(it) }
    }

    private fun fullWidth(): LinearLayout.LayoutParams =
        LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT,
            LinearLayout.LayoutParams.WRAP_CONTENT
        )

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()
}
