package com.zz6zz666.seulabor

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.LinearLayout
import androidx.appcompat.app.AppCompatActivity
import org.json.JSONArray

class CourseActivity : AppCompatActivity() {
    companion object {
        private const val COURSE_PAGE = "https://labor.seu.edu.cn/SJItemKaiKe/XuanKe/Index"
        private const val LOGIN_PAGE_SCRIPT = """
            (function () {
              if (document.getElementById('c_app_page_index_XuanKe_table')) return false;
              if (!document.querySelector('input[type=password]')) return false;
              var text = document.body ? document.body.innerText : '';
              return text.indexOf('统一认证登录') >= 0 || text.indexOf('用户登录') >= 0;
            })()
        """
    }

    private var loginLaunched = false
    private lateinit var web: WebView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        web = WebView(this).apply {
            setBackgroundColor(getColor(R.color.app_surface))
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.useWideViewPort = true
            settings.loadWithOverviewMode = true
            settings.builtInZoomControls = true
            settings.displayZoomControls = false
            settings.userAgentString =
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 " +
                    "(KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36"
            webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(view: WebView, url: String?): Boolean = false

                override fun onPageFinished(view: WebView, url: String?) {
                    maybeSwitchToCas(url)
                }
            }
        }
        if (!injectCookies()) {
            openCasLogin()
            return
        }
        web.loadUrl(COURSE_PAGE)

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(getColor(R.color.app_background))
        }
        root.addView(web, ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        setContentView(root)
    }

    /**
     * The labor site serves its own local login form before CAS. That form is a
     * dead end for the WebView: only the CAS page has the auto-fill contract.
     */
    private fun maybeSwitchToCas(url: String?) {
        val uri = url?.let { Uri.parse(it) } ?: return
        val host = uri.host ?: return
        if (host != "labor.seu.edu.cn" && !host.endsWith(".labor.seu.edu.cn")) return
        if (loginLaunched || isFinishing) return
        if (uri.path?.startsWith("/AuthServer/Login") == true) {
            openCasLogin()
            return
        }
        web.postDelayed({
            if (loginLaunched || isFinishing) return@postDelayed
            web.evaluateJavascript(LOGIN_PAGE_SCRIPT) { result ->
                if (result == "true") openCasLogin()
            }
        }, 700L)
    }

    private fun openCasLogin() {
        if (loginLaunched || isFinishing) return
        loginLaunched = true
        startActivity(
            Intent(this, LoginActivity::class.java)
                .putExtra(LoginActivity.EXTRA_RETURN_TO_COURSE, true)
        )
        finish()
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        Nav.backToMain(this)
    }

    private fun injectCookies(): Boolean {
        val raw = runCatching { CoreBridge.nativeExportCookies() }.getOrNull() ?: return false
        val array = runCatching { JSONArray(raw) }.getOrNull() ?: return false
        if (array.length() == 0) return false
        val manager = CookieManager.getInstance()
        for (i in 0 until array.length()) {
            val cookie = array.optJSONObject(i) ?: continue
            val domain = cookie.optString("domain")
            if (domain.isEmpty()) continue
            val name = cookie.optString("name")
            val value = cookie.optString("value")
            val hosts = if (domain == "seu.edu.cn" || domain.endsWith(".seu.edu.cn")) {
                listOf("labor.seu.edu.cn", "auth.seu.edu.cn")
            } else {
                listOf(domain)
            }
            for (host in hosts) {
                manager.setCookie("https://$host/", "$name=$value")
            }
        }
        manager.flush()
        return true
    }
}
