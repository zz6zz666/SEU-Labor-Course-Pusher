package com.zz6zz666.seulabor

import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.ConcurrentHashMap

class LoginActivity : Activity() {
    companion object {
        const val EXTRA_RETURN_TO_COURSE = "return_to_course"
        private const val CAS_URL =
            "https://auth.seu.edu.cn/dist/#/dist/main/login" +
                "?service=https://labor.seu.edu.cn/UnifiedAuth/CASLogin"
    }

    private lateinit var web: WebView
    private var username = ""
    private var password = ""
    private var completing = false
    private val capturedCookies = ConcurrentHashMap<String, String>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val config = runCatching { JSONObject(CoreBridge.nativeGetConfig()) }.getOrNull()
        val credentials = config?.optJSONObject("credentials")
        username = credentials?.optString("username", "") ?: ""
        password = credentials?.optString("password", "") ?: ""

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.WHITE)
        }
        val bar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(12), dp(8), dp(12), dp(8))
        }
        bar.addView(TextView(this).apply {
            text = "完成登录后点右侧按钮"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTextColor(Color.parseColor("#5B6270"))
        }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        bar.addView(Button(this).apply {
            text = "已完成登录"
            isAllCaps = false
            setOnClickListener { completeLogin() }
        })
        root.addView(bar, LinearLayout.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)

        web = WebView(this).apply {
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.useWideViewPort = true
            settings.loadWithOverviewMode = true
            // The site rejects some requests unless a desktop Chrome UA is sent.
            settings.userAgentString =
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 " +
                    "(KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36"
            CookieManager.getInstance().setAcceptThirdPartyCookies(this, true)
            webViewClient = object : WebViewClient() {
                override fun onPageFinished(view: WebView, url: String?) {
                    val host = url?.let { Uri.parse(it).host } ?: ""
                    // The CAS URL carries the service URL in its query string, so
                    // matching the substring would treat the login page itself as
                    // the course page and skip the auto-fill entirely.
                    if (host == "labor.seu.edu.cn" || host.endsWith(".labor.seu.edu.cn")) {
                        completeLogin()
                    } else {
                        injectFill(view)
                    }
                }

                override fun shouldInterceptRequest(
                    view: WebView,
                    request: WebResourceRequest
                ): WebResourceResponse? {
                    val host = request.url.host
                    if (host != null && host.endsWith("seu.edu.cn")) {
                        request.requestHeaders["Cookie"]?.let { capturedCookies[host] = it }
                    }
                    return null
                }
            }
        }
        root.addView(web, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
        setContentView(root)
        web.loadUrl(CAS_URL)
    }

    private fun injectFill(view: WebView, attempt: Int = 0) {
        if (attempt > 8 || username.isEmpty() || password.isEmpty() || isFinishing) return
        val script = CoreBridge.nativeFillScript(username, password)
        if (script.isEmpty()) return
        // The CAS page is a hydrated SPA; onPageFinished often fires before the
        // React form exists, so retry until the script reports success.
        view.postDelayed({
            if (isFinishing || isDestroyed) return@postDelayed
            view.evaluateJavascript(script) { result ->
                if (result != "true") injectFill(view, attempt + 1)
            }
        }, if (attempt == 0) 600L else 1_200L)
    }

    private fun completeLogin(attempt: Int = 0) {
        if (completing || isFinishing) return
        completing = true
        CookieManager.getInstance().flush()
        val cookies = collectCookies()
        if (cookies.length() == 0) {
            completing = false
            if (attempt < 4) {
                web.postDelayed({ completeLogin(attempt + 1) }, 1_500L)
            } else {
                toast("没有读取到登录 Cookie，请确认已登录成功")
            }
            return
        }
        val payload = cookies.toString()
        Thread({
            val imported = runCatching { CoreBridge.nativeImportCookies(payload) }
                .getOrDefault(false)
            val probe = runCatching { JSONObject(CoreBridge.nativeProbe()) }.getOrNull()
            runOnUiThread {
                if (imported && probe?.optString("verdict") == "courses") {
                    toast("登录成功")
                    startService(
                        Intent(this, MonitorService::class.java)
                            .setAction(MonitorService.ACTION_FETCH_NOW)
                    )
                    if (intent.getBooleanExtra(EXTRA_RETURN_TO_COURSE, false)) {
                        startActivity(Intent(this, CourseActivity::class.java))
                        finish()
                    } else {
                        Nav.backToMain(this)
                    }
                } else {
                    completing = false
                    if (attempt < 4) {
                        web.postDelayed({ completeLogin(attempt + 1) }, 1_500L)
                    } else {
                        val reason = probe?.optString("reason", "") ?: ""
                        toast("已获取 Cookie，但会话未生效：$reason")
                    }
                }
            }
        }, "login-probe").start()
    }

    private fun collectCookies(): JSONArray {
        val array = JSONArray()
        for (url in listOf("https://labor.seu.edu.cn", "https://auth.seu.edu.cn")) {
            val host = Uri.parse(url).host ?: continue
            val raw = capturedCookies[host] ?: CookieManager.getInstance().getCookie(url) ?: continue
            raw.split(';').forEach { part ->
                val kv = part.trim()
                val index = kv.indexOf('=')
                if (index <= 0) return@forEach
                array.put(JSONObject().apply {
                    put("name", kv.substring(0, index))
                    put("value", kv.substring(index + 1))
                    put("domain", host)
                    put("path", "/")
                    put("expires", 0)
                    put("secure", true)
                    put("httpOnly", true)
                })
            }
        }
        return array
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        Nav.backToMain(this)
    }

    private fun toast(text: String) {
        Toast.makeText(this, text, Toast.LENGTH_LONG).show()
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()
}
