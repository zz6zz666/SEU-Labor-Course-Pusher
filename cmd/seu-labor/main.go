// Command seu-labor is the resident SEU labor-course watcher.
//
// Polling, auto-selection and auto-cancel use plain HTTP (the list is
// server-rendered). The system Chromium browser is launched only for
// interactive login and course viewing.
package main

import (
	"context"
	"flag"
	"fmt"
	"net/url"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"sync"
	"syscall"
	"time"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/autostart"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/browser"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/config"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/logging"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/login"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/notify"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/osutil"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/paths"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/selection"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/session"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/singleinstance"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/site"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/state"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/ui"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/watcher"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/wizard"
)

var version = "dev"

func main() {
	// Must run before any window is created so the WebView2 settings window
	// renders crisply on scaled (high-DPI) displays.
	osutil.EnablePerMonitorDPI()

	var (
		once    = flag.Bool("once", false, "抓取一次后退出(用于验证配置与登录态)")
		doLogin = flag.Bool("login", false, "打开登录窗口,完成后保存登录态并退出")
		showWiz = flag.Bool("wizard", false, "启动后直接打开设置向导(安装器使用)")
		showVer = flag.Bool("version", false, "打印版本后退出")

		wizUI   = flag.Bool("wizard-ui", false, "内部使用:仅运行设置向导窗口(短进程)")
		wizURL  = flag.String("wiz-url", "", "内部使用:设置向导地址")
		wizData = flag.String("wiz-data", "", "内部使用:设置向导 WebView2 数据目录")
	)
	flag.Parse()

	if *showVer {
		fmt.Println(version)
		return
	}
	if *wizUI {
		if err := runWizardUI(*wizURL, *wizData); err != nil {
			fmt.Fprintln(os.Stderr, "错误:", err)
			os.Exit(1)
		}
		return
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	if err := run(ctx, *once, *doLogin, *showWiz); err != nil {
		fmt.Fprintln(os.Stderr, "错误:", err)
		os.Exit(1)
	}
}

func run(ctx context.Context, once, doLogin, showWizard bool) error {
	p, err := paths.Resolve()
	if err != nil {
		return err
	}

	store, warnings, err := config.Open(p.ConfigPath)
	if err != nil {
		return err
	}
	cfg := store.Get()
	logging.Init(p.LogDir, logging.ParseLevel(cfg.Logging.Level), cfg.Logging.RetentionDays)
	log := logging.New("main")
	if err := osutil.SetAppUserModelID(osutil.AppUserModelID); err != nil {
		log.Warn("设置 AppUserModelID 失败:", err)
	}

	log.Info("================ SEU 劳动教育课程监控 ================")
	log.Info("版本", version, "·", p.DataDir)
	log.Info("运行配置:", cfg.SafeSummary())
	for _, w := range warnings {
		log.Warn(w)
	}

	// Repair the autostart entry on every launch when it is enabled: the stored
	// path can go stale across upgrades, or have been written malformed by an
	// older build (which silently broke startup-at-login).
	if cfg.Behavior.AutoLaunchAtLogin {
		actual := autostart.Apply(true)
		if actual != cfg.Behavior.AutoLaunchAtLogin {
			_ = store.Update(func(c *config.Config) { c.Behavior.AutoLaunchAtLogin = actual })
		}
	}

	st, err := state.Open(p.StatePath)
	if err != nil {
		return err
	}

	client, err := session.New(p.CookiesPath, 20*time.Second)
	if err != nil {
		return err
	}

	if doLogin {
		return runLogin(ctx, p, store, client, st, log)
	}
	if client.Jar.IsEmpty() {
		if cfg.Behavior.AutoLogin && cfg.HasCredentials() {
			log.Info("尚无登录态,将由后台尝试静默登录")
		} else {
			log.Warn("尚未登录:请先运行 `seu-labor -login` 完成一次登录")
		}
	}

	dispatcher := notify.NewDispatcher(log)
	dispatcher.Add(notify.NewPushPlus(store, log))
	dispatcher.Add(notify.NewToast(store, log))

	selector := &selection.Runner{Client: client}
	w := watcher.New(store, st, client, dispatcher, logging.New("watcher"), selector)

	// trySilentLogin recovers an expired session without any UI, using the
	// stored credentials against the CAS page. It returns false (leaving the
	// session untouched) when the server demands a captcha/SMS or it fails.
	loginMu := &sync.Mutex{}
	trySilentLogin := func() bool {
		if !loginMu.TryLock() {
			return false
		}
		defer loginMu.Unlock()
		cfg := store.Get()
		if !cfg.Behavior.AutoLogin || !cfg.HasCredentials() {
			return false
		}
		res := login.AutoRun(ctx, login.AutoOptions{
			Username: cfg.Credentials.Username,
			Password: cfg.Credentials.Password,
			Log:      logging.New("login"),
		})
		if res.Outcome != login.AutoSuccess {
			log.Warn("静默重登未成功:", string(res.Outcome), res.Detail)
			return false
		}
		u, _ := url.Parse(session.CoursePage)
		client.Jar.SetCookies(u, res.Cookies)
		if err := client.Jar.Save(); err != nil {
			log.Warn("保存登录态失败:", err)
		}
		_ = st.Update(func(s *state.State) {
			s.FirstRunCompleted = true
			s.AuthState = state.AuthValid
		})
		log.Info("静默重登成功")
		return true
	}

	// openLoginWindow runs the interactive login (captcha/SMS). Callers run it
	// in a goroutine.
	openLoginWindow := func() {
		if err := runLogin(ctx, p, store, client, st, log); err != nil {
			log.Warn("登录失败:", err)
			return
		}
		w.TriggerNow()
	}
	openLoginAsync := func() { go openLoginWindow() }

	if once {
		tickCtx, cancel := context.WithTimeout(ctx, 40*time.Second)
		defer cancel()
		w.Tick(tickCtx)
		s := w.Status()
		log.Info("单次抓取结果:", s.LastVerdict, "-", s.LastMessage)
		return client.Jar.Save()
	}

	inst, already, err := singleinstance.Acquire()
	if err != nil {
		log.Warn("单实例锁初始化失败:", err)
	}
	if already {
		log.Info("检测到已有实例在运行,请求其打开设置向导后退出")
		_ = singleinstance.SignalExisting()
		return nil
	}
	if inst != nil {
		defer inst.Release()
	}

	var tray *ui.Tray
	var wizardSrv *wizard.Server
	var wizardProc *exec.Cmd
	var wizardMu sync.Mutex

	// killWizard tears down the settings API server and the short-lived window
	// process that renders it. The caller must hold wizardMu.
	killWizard := func() {
		proc, srv := wizardProc, wizardSrv
		wizardProc, wizardSrv = nil, nil
		if proc != nil && proc.Process != nil {
			_ = proc.Process.Kill()
		}
		if srv != nil {
			srv.SetOnClose(nil)
			_ = srv.Close()
		}
	}
	// killWizardProc stops only the window process. It takes the lock itself so
	// it is safe to call from the server's close callback (any goroutine).
	killWizardProc := func() {
		wizardMu.Lock()
		proc := wizardProc
		wizardProc = nil
		wizardMu.Unlock()
		if proc != nil && proc.Process != nil {
			_ = proc.Process.Kill()
		}
	}

	cleanup := func() {
		wizardMu.Lock()
		killWizard()
		wizardMu.Unlock()
		courseViewMu.Lock()
		if courseView != nil {
			_ = courseView.Close()
			courseView = nil
		}
		courseViewMu.Unlock()
		_ = client.Jar.Save()
	}
	quit := func() {
		if tray != nil {
			tray.Stop()
		}
		cleanup()
		os.Exit(0)
	}

	openWizard := func() {
		wizardMu.Lock()
		defer wizardMu.Unlock()
		killWizard()

		srv, err := wizard.Open(wizardActions(p, store, st, w, client, log, openLoginAsync), log)
		if err != nil {
			log.Warn("启动设置向导失败:", err)
			return
		}
		wizardSrv = srv

		// The WebView2 window runs in a separate, short-lived process (this same
		// executable with -wizard-ui) so the resident daemon never loads the
		// WebView2 runtime: closing the window exits the child and reclaims all
		// of its memory. Every wizard action still runs here, behind the
		// loopback API the window talks to.
		exe, err := os.Executable()
		if err != nil {
			log.Warn("定位程序路径失败,改用默认浏览器:", err)
			_ = osutil.OpenURL(srv.URL())
			return
		}
		proc := exec.Command(exe,
			"-wizard-ui",
			"-wiz-url", srv.URL(),
			"-wiz-data", filepath.Join(p.DataDir, "wizard-webview"),
		)
		if err := proc.Start(); err != nil {
			log.Warn("打开设置向导窗口失败,改用默认浏览器:", err)
			_ = osutil.OpenURL(srv.URL())
			return
		}
		wizardProc = proc

		// The page's「关闭窗口」button closes the API server; also stop the child.
		srv.SetOnClose(killWizardProc)

		// The child exiting tears the API server down.
		go func(proc *exec.Cmd, srv *wizard.Server) {
			_ = proc.Wait()
			wizardMu.Lock()
			if wizardProc == proc {
				wizardProc = nil
			}
			if wizardSrv == srv {
				wizardSrv = nil
			}
			wizardMu.Unlock()
			_ = srv.Close()
		}(proc, srv)

		log.Info("设置向导已启动(独立窗口进程)")
	}

	statusLines := func() []string {
		s := w.Status()
		var login string
		switch st.Get().AuthState {
		case state.AuthValid:
			login = "登录正常"
		case state.AuthExpired:
			login = "登录失效"
		default:
			login = "尚未登录"
		}
		first := fmt.Sprintf("%s · 符合条件 %d 门 · 今日新推送 %d 门", login, s.CurrentValid, s.PushedToday)
		second := s.LastMessage
		if second == "" {
			second = "等待首次抓取"
		}
		if !s.NextRun.IsZero() {
			second += fmt.Sprintf("（下次 %s）", s.NextRun.Format("15:04:05"))
		}
		return []string{first, second}
	}

	actions := ui.Actions{
		StatusText: func() string {
			return statusLines()[0]
		},
		StatusLines:  statusLines,
		Version:      version,
		OnOpenCourse: func() { go openCourseView(p, client, log) },
		OnLogin:      openLoginAsync,
		OnFetchNow:   w.TriggerNow,
		OnOpenWizard: openWizard,
		OnOpenConfig: func() { _ = osutil.OpenTarget(p.ConfigPath) },
		OnOpenLogs:   func() { _ = osutil.OpenFolder(p.LogDir) },
		IsAutoStart:  autostart.IsEnabled,
		ToggleAutoStart: func() {
			actual := autostart.Apply(!autostart.IsEnabled())
			_ = store.Update(func(c *config.Config) { c.Behavior.AutoLaunchAtLogin = actual })
		},
		IsAutoSelect: func() bool { return store.Get().Behavior.AutoSelect },
		ToggleAutoSelect: func() {
			_ = store.Update(func(c *config.Config) { c.Behavior.AutoSelect = !c.Behavior.AutoSelect })
		},
		OnQuit: quit,
	}

	tray, err = ui.NewTray(actions, p.DataDir)
	if err != nil {
		log.Warn("创建系统托盘失败,将以无界面方式常驻:", err)
		tray = nil
	}

	w.SetOnAuthExpired(func(reason string) {
		go func() {
			if trySilentLogin() {
				w.TriggerNow()
				return
			}
			// Silent recovery failed: now it is genuinely a human's turn.
			dispatcher.Dispatch(ctx, notify.BuildAuthExpired(reason))
			if store.Get().Behavior.AutoOpenOnAuthFailure {
				log.Info("静默重登不可行,自动打开登录窗口")
				openLoginWindow()
			}
		}()
	})

	go w.Run(ctx)

	// While the session is expired, retry the unattended login periodically.
	// This covers the case where the interactive window timed out or was closed.
	go func() {
		t := time.NewTicker(5 * time.Minute)
		defer t.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-t.C:
				if st.Get().AuthState != state.AuthExpired {
					continue
				}
				if trySilentLogin() {
					w.TriggerNow()
				}
			}
		}
	}()

	if inst != nil {
		go func() {
			for {
				if err := inst.Wait(); err != nil {
					return
				}
				log.Info("收到另一实例的请求,打开设置向导")
				openWizard()
			}
		}()
	}

	go func() {
		t := time.NewTicker(2 * time.Second)
		defer t.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-t.C:
				changed, warns, err := store.Reload()
				if err != nil || !changed {
					continue
				}
				cfg := store.Get()
				logging.Configure(logging.ParseLevel(cfg.Logging.Level), cfg.Logging.RetentionDays)
				log.Info("配置已热重载", cfg.SafeSummary())
				for _, w := range warns {
					log.Warn(w)
				}
				if tray != nil {
					tray.Update()
				}
			}
		}
	}()

	if tray != nil {
		go func() {
			t := time.NewTicker(3 * time.Second)
			defer t.Stop()
			for {
				select {
				case <-ctx.Done():
					return
				case <-t.C:
					tray.Update()
				}
			}
		}()
	}

	if !st.Get().FirstRunCompleted {
		log.Info("首次运行,打开设置向导")
		go openWizard()
	} else if showWizard {
		log.Info("按启动参数打开设置向导")
		go openWizard()
	}

	go func() {
		<-ctx.Done()
		quit()
	}()

	if tray == nil {
		<-ctx.Done()
		cleanup()
		return nil
	}
	return tray.Run()
}

func runLogin(ctx context.Context, p paths.Paths, store *config.Store, client *session.Client, st *state.Store, log *logging.Logger) error {
	cred := store.Get().Credentials
	// Open the unified-auth (CAS) page directly: the course page redirects to
	// the labor-local AuthServer/Login, which needs an account we do not have.
	cookies, err := login.Run(ctx, login.Options{
		URL:        session.CasLogin,
		ProfileDir: filepath.Join(p.DataDir, "browser-profile"),
		Timeout:    5 * time.Minute,
		Username:   cred.Username,
		Password:   cred.Password,
		Log:        logging.New("login"),
	})
	if err != nil {
		return err
	}

	u, _ := url.Parse(session.CoursePage)
	client.Jar.SetCookies(u, cookies)
	if err := client.Jar.Save(); err != nil {
		return err
	}
	_ = st.Update(func(s *state.State) {
		s.FirstRunCompleted = true
		s.AuthState = state.AuthValid
	})
	log.Info("登录态已保存,后台现在可以直接轮询")
	return nil
}

var (
	courseViewMu sync.Mutex
	courseView   *browser.Chrome
)

func openCourseView(p paths.Paths, client *session.Client, log *logging.Logger) {
	courseViewMu.Lock()
	defer courseViewMu.Unlock()

	if courseView != nil {
		if err := courseView.Navigate(session.CoursePage); err == nil {
			return
		}
		_ = courseView.Close()
		courseView = nil
	}

	win, err := browser.Launch(context.Background(), browser.LaunchOptions{
		Visible:    true,
		ProfileDir: filepath.Join(p.DataDir, "browser-profile-view"),
		AppURL:     "about:blank",
	})
	if err != nil {
		log.Warn("打开选课页失败:", err)
		return
	}
	if err := win.SetCookies(client.Jar.All()); err != nil {
		log.Warn("注入登录态失败:", err)
	}
	if err := win.Navigate(session.CoursePage); err != nil {
		log.Warn("加载选课页失败:", err)
	}
	courseView = win
}

func wizardActions(p paths.Paths, store *config.Store, st *state.Store, w *watcher.Watcher, client *session.Client, log *logging.Logger, openLogin func()) wizard.Actions {
	return wizard.Actions{
		Status: func() wizard.Status {
			cfg := store.Get()
			s := w.Status()
			return wizard.Status{
				FirstRunCompleted:     st.Get().FirstRunCompleted,
				CredentialsConfigured: cfg.HasCredentials(),
				PushplusConfigured:    cfg.Push.PushPlus.Token != "",
				WindowsNotifyEnabled:  cfg.Push.Windows.Enabled,
				AuthState:             string(st.Get().AuthState),
				WatcherMessage:        s.LastMessage,
				LastSuccessAt:         s.LastSuccessAt,
				AutoStartEnabled:      autostart.IsEnabled(),
				AutoSelectEnabled:     cfg.Behavior.AutoSelect,
				FiltersConfigured:     len(cfg.Filters.Locations) > 0 || len(cfg.Filters.Categories) > 0,
				ConfigPath:            p.ConfigPath,
				DataDir:               p.DataDir,
				Version:               version,
			}
		},
		Config: func() wizard.ConfigView {
			cfg := store.Get()
			return wizard.ConfigView{
				Username:             cfg.Credentials.Username,
				Password:             cfg.Credentials.Password,
				PushplusToken:        cfg.Push.PushPlus.Token,
				PushplusEnabled:      cfg.Push.PushPlus.Enabled,
				WindowsNotifyEnabled: cfg.Push.Windows.Enabled,
				AutoLaunchAtLogin:    cfg.Behavior.AutoLaunchAtLogin,
				AutoSelect:           cfg.Behavior.AutoSelect,
				Locations:            cfg.Filters.Locations,
				Categories:           cfg.Filters.Categories,
			}
		},
		Verify: func(c context.Context) (string, string) {
			probe, _ := site.Check(c, client)
			switch probe.Verdict {
			case site.VerdictCourses:
				return "valid", probe.Reason
			case site.VerdictLogin:
				return "expired", probe.Reason
			default:
				return "unknown", probe.Reason
			}
		},
		SaveAccount: func(username, password string) error {
			return store.Update(func(c *config.Config) {
				c.Credentials.Username = username
				c.Credentials.Password = password
			})
		},
		SaveFilters: func(locations, categories []string) error {
			return store.Update(func(c *config.Config) {
				c.Filters.Locations = locations
				c.Filters.Categories = categories
			})
		},
		SaveNotify: func(token string, pushplusEnabled, windowsEnabled bool) error {
			return store.Update(func(c *config.Config) {
				c.Push.PushPlus.Token = token
				c.Push.PushPlus.Enabled = pushplusEnabled && token != ""
				c.Push.Windows.Enabled = windowsEnabled
			})
		},
		SetBehavior: func(autoStart, autoSelect *bool) error {
			if autoStart != nil {
				actual := autostart.Apply(*autoStart)
				if err := store.Update(func(c *config.Config) { c.Behavior.AutoLaunchAtLogin = actual }); err != nil {
					return err
				}
			}
			if autoSelect != nil {
				if err := store.Update(func(c *config.Config) { c.Behavior.AutoSelect = *autoSelect }); err != nil {
					return err
				}
			}
			return nil
		},
		OpenLogin:    openLogin,
		OpenCourse:   func() { go openCourseView(p, client, log) },
		OpenConfig:   func() { _ = osutil.OpenTarget(p.ConfigPath) },
		OpenLogs:     func() { _ = osutil.OpenFolder(p.LogDir) },
		OpenExternal: osutil.OpenURL,
	}
}
