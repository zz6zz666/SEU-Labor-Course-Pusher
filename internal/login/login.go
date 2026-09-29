// Package login owns the two ways to establish a session.
//
//   - AutoRun: unattended and browser-free. It replays the CAS login flow over
//     plain HTTP (public key → RSA-encrypt password → casLogin → redeem ticket)
//     and only asks a human when the server actually demands a captcha/SMS.
//   - Run: interactive, visible. It opens the CAS page in a browser window so
//     the user can clear a captcha/SMS, then harvests the cookie jar.
package login

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/browser"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/logging"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/session"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/site"
)

// CAS backend endpoints (reversed from the Umi SPA bundle and a live capture).
const (
	casService     = "https://labor.seu.edu.cn/UnifiedAuth/CASLogin"
	casBase        = "https://auth.seu.edu.cn/auth/casback"
	casNeedCaptcha = casBase + "/needCaptcha"
	casChiperKey   = casBase + "/getChiperKey"
	casLoginURL    = casBase + "/casLogin"
)

// ReadyProbe is the shape of browser.ReadyScript's result.
type ReadyProbe struct {
	Ready       bool   `json:"ready"`
	HasUsername bool   `json:"hasUsername"`
	HasPassword bool   `json:"hasPassword"`
	HasButton   bool   `json:"hasButton"`
	NeedStage2  bool   `json:"needStage2"`
	HasCaptcha  bool   `json:"hasCaptcha"`
	Text        string `json:"text"`
}

func probeForm(b browser.Browser) (ReadyProbe, bool) {
	raw, err := b.Eval(browser.ReadyScript())
	if err != nil {
		return ReadyProbe{}, false
	}
	var p ReadyProbe
	if json.Unmarshal(raw, &p) != nil {
		return ReadyProbe{}, false
	}
	return p, p.Ready || p.NeedStage2 || p.HasCaptcha
}

// ---------------------------------------------------------------- unattended

type AutoOptions struct {
	Username string
	Password string
	Log      *logging.Logger
}

type AutoOutcome string

const (
	AutoSuccess       AutoOutcome = "success"
	AutoNoCredentials AutoOutcome = "noCredentials"
	AutoNeedManual    AutoOutcome = "needManual"
	AutoError         AutoOutcome = "error"
)

type AutoResult struct {
	Outcome AutoOutcome
	Detail  string
	Cookies []*http.Cookie
}

// AutoRun replays the CAS login over HTTP and returns the harvested cookies.
// It probes for a captcha/SMS first and returns AutoNeedManual (making no
// login attempt) when a human would be required.
func AutoRun(ctx context.Context, opts AutoOptions) AutoResult {
	if opts.Username == "" || opts.Password == "" {
		return AutoResult{Outcome: AutoNoCredentials, Detail: "未配置账号密码"}
	}
	if opts.Log == nil {
		opts.Log = logging.New("login")
	}

	jar := session.EmptyJar()
	client := session.NewClient(jar, 25*time.Second)

	// 0) Ask the server whether a captcha / SMS would be demanded.
	capRes, err := client.Get(ctx, casNeedCaptcha)
	if err != nil {
		return AutoResult{Outcome: AutoError, Detail: "探测验证码失败: " + err.Error()}
	}
	var capInfo struct {
		Success    bool   `json:"success"`
		Info       string `json:"info"`
		NeedStage2 bool   `json:"needStage2Validation"`
	}
	_ = json.Unmarshal([]byte(capRes.Body), &capInfo)
	if capInfo.NeedStage2 {
		return AutoResult{Outcome: AutoNeedManual, Detail: "服务端要求短信二次验证"}
	}
	if !capInfo.Success || (strings.Contains(capInfo.Info, "验证码") && !strings.Contains(capInfo.Info, "不需要")) {
		return AutoResult{Outcome: AutoNeedManual, Detail: "登录需要验证码: " + capInfo.Info}
	}

	// 1) Fetch the RSA public key.
	keyRes, err := client.PostJSON(ctx, casChiperKey, []byte("{}"))
	if err != nil {
		return AutoResult{Outcome: AutoError, Detail: "获取公钥失败: " + err.Error()}
	}
	var keyInfo struct {
		Success   bool   `json:"success"`
		PublicKey string `json:"publicKey"`
	}
	_ = json.Unmarshal([]byte(keyRes.Body), &keyInfo)
	if !keyInfo.Success || keyInfo.PublicKey == "" {
		return AutoResult{Outcome: AutoError, Detail: "获取公钥失败"}
	}
	encPwd, err := rsaEncryptBase64(keyInfo.PublicKey, opts.Password)
	if err != nil {
		return AutoResult{Outcome: AutoError, Detail: "加密密码失败: " + err.Error()}
	}

	// 2) Submit the credentials.
	payload, _ := json.Marshal(map[string]any{
		"service":        casService,
		"username":       opts.Username,
		"password":       encPwd,
		"captcha":        "",
		"rememberMe":     false,
		"loginType":      "account",
		"wxBinded":       false,
		"mobilePhoneNum": "",
		"fingerPrint":    "stable_" + deviceFingerprint(),
	})
	loginRes, err := client.PostJSON(ctx, casLoginURL, payload)
	if err != nil {
		return AutoResult{Outcome: AutoError, Detail: "提交登录失败: " + err.Error()}
	}
	var loginInfo struct {
		Code        int    `json:"code"`
		Success     bool   `json:"success"`
		Info        string `json:"info"`
		RedirectURL string `json:"redirectUrl"`
	}
	_ = json.Unmarshal([]byte(loginRes.Body), &loginInfo)
	if loginInfo.Code != 200 || loginInfo.RedirectURL == "" {
		return AutoResult{Outcome: AutoError, Detail: fmt.Sprintf("CAS 登录失败(code=%d): %s", loginInfo.Code, loginInfo.Info)}
	}

	// 3) Redeem the ticket at the service so the labor session cookies are set.
	// The CAS response returns redirectUrl percent-encoded.
	serviceURL, err := url.QueryUnescape(loginInfo.RedirectURL)
	if err != nil || !strings.HasPrefix(serviceURL, "http") {
		return AutoResult{Outcome: AutoError, Detail: "CAS 未返回有效跳转地址"}
	}
	if _, err := client.Get(ctx, serviceURL); err != nil {
		return AutoResult{Outcome: AutoError, Detail: "兑换票据失败: " + err.Error()}
	}

	// 4) Verify the session actually works.
	probe, _ := site.Check(ctx, client)
	if probe.Verdict != site.VerdictCourses {
		return AutoResult{Outcome: AutoError, Detail: "登录后会话未生效: " + probe.Reason}
	}
	return AutoResult{Outcome: AutoSuccess, Detail: fmt.Sprintf("静默登录成功(%d 个 Cookie)", len(jar.All())), Cookies: jar.All()}
}

// rsaEncryptBase64 parses the site's public key (URL-safe base64 DER) and
// returns the PKCS#1 v1.5 ciphertext as standard base64.
func rsaEncryptBase64(pubKey, plaintext string) (string, error) {
	der, err := decodeBase64Any(pubKey)
	if err != nil {
		return "", err
	}
	parsed, err := x509.ParsePKIXPublicKey(der)
	if err != nil {
		return "", err
	}
	pub, ok := parsed.(*rsa.PublicKey)
	if !ok {
		return "", fmt.Errorf("公钥类型不是 RSA")
	}
	enc, err := rsa.EncryptPKCS1v15(rand.Reader, pub, []byte(plaintext))
	if err != nil {
		return "", err
	}
	return base64.StdEncoding.EncodeToString(enc), nil
}

func decodeBase64Any(s string) ([]byte, error) {
	for _, enc := range []*base64.Encoding{
		base64.RawURLEncoding, base64.URLEncoding, base64.RawStdEncoding, base64.StdEncoding,
	} {
		if b, err := enc.DecodeString(s); err == nil {
			return b, nil
		}
	}
	return nil, fmt.Errorf("无法解码公钥")
}

// deviceFingerprint returns a stable 32-hex-char id, mirroring the SPA's
// "stable_<hex>" fingerprint.
func deviceFingerprint() string {
	buf := make([]byte, 16)
	if _, err := rand.Read(buf); err != nil {
		return "00000000000000000000000000000000"
	}
	return fmt.Sprintf("%x", buf)
}

// ---------------------------------------------------------------- interactive

type Options struct {
	URL          string
	ProfileDir   string
	Timeout      time.Duration
	PollInterval time.Duration
	// Username/Password pre-fill the login form (the captcha is left to the user).
	Username string
	Password string
	Log      *logging.Logger
}

// Run blocks until the session authenticates or the deadline passes. The
// returned cookies include session cookies; the caller must persist them.
func Run(ctx context.Context, opts Options) ([]*http.Cookie, error) {
	if opts.PollInterval <= 0 {
		opts.PollInterval = 3 * time.Second
	}
	if opts.Log == nil {
		opts.Log = logging.New("login")
	}

	b, err := browser.Launch(ctx, browser.LaunchOptions{
		Visible:    true,
		ProfileDir: opts.ProfileDir,
		AppURL:     opts.URL,
	})
	if err != nil {
		return nil, err
	}
	defer b.Close()

	opts.Log.Info("已打开登录窗口,请在窗口内完成登录(可能需要验证码/短信)")

	ticker := time.NewTicker(opts.PollInterval)
	defer ticker.Stop()
	deadline := time.Now().Add(opts.Timeout)
	filled := opts.Username == "" || opts.Password == ""

	for {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-ticker.C:
		}

		if time.Now().After(deadline) {
			return nil, fmt.Errorf("等待登录超时(%s)", opts.Timeout)
		}

		if !filled {
			filled = tryFillCredentials(b, opts)
		}

		cookies, err := b.AllCookies()
		if err != nil {
			continue
		}
		if authenticated(ctx, cookies) {
			opts.Log.Info("检测到登录成功,已获取", len(cookies), "个 Cookie")
			return cookies, nil
		}
	}
}

// tryFillCredentials pre-fills the login form once it renders. Returns true
// once the credentials have been written (or were already present).
func tryFillCredentials(b browser.Browser, opts Options) bool {
	probe, ok := probeForm(b)
	if !ok || !probe.Ready {
		return false
	}
	raw, err := b.Eval(browser.FillCredentialsScript(opts.Username, opts.Password))
	if err != nil {
		return false
	}
	var filled bool
	_ = json.Unmarshal(raw, &filled)
	if filled {
		opts.Log.Info("已自动预填账号密码,请填写验证码后点击登录")
	}
	return filled
}

func authenticated(ctx context.Context, cookies []*http.Cookie) bool {
	jar := session.EmptyJar()
	u, _ := url.Parse(session.CoursePage)
	jar.SetCookies(u, cookies)
	probe, _ := site.Check(ctx, session.NewClient(jar, 15*time.Second))
	return probe.Verdict == site.VerdictCourses
}
