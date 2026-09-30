//! The polling state machine: fetch, classify, parse, diff, notify, back off.
//! It relies on HTTP only; the course list is server-rendered.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use chrono::{Local, Timelike};

use crate::config::Store;
use crate::logging::Logger;
use crate::notify::{self, Dispatcher};
use crate::selection::{self, AutoSelector};
use crate::session;
use crate::site::{self, Course};
use crate::state;

#[derive(Clone)]
pub struct Status {
    pub last_verdict: String,
    pub last_message: String,
    pub current_valid: usize,
    pub pushed_today: i64,
    pub failures: i64,
    pub next_run: Option<chrono::DateTime<Local>>,
    pub last_success_at: String,
}

struct DayStats {
    date: String,
    ticks: i64,
    successes: i64,
    pushed_new: i64,
}

struct Inner {
    failures: i64,
    auth_alerted: bool,
    stats: DayStats,
    status: Status,
}

type AuthExpiredFn = Arc<dyn Fn(String) + Send + Sync>;

pub struct Watcher {
    store: Arc<Store>,
    state: Arc<state::Store>,
    client: Arc<session::Client>,
    notify: Arc<Dispatcher>,
    log: Logger,
    selector: Option<Arc<dyn AutoSelector>>,
    inner: Mutex<Inner>,
    trigger_mu: Mutex<bool>,
    trigger_cv: Condvar,
    stop: AtomicBool,
    on_auth_expired: Mutex<Option<AuthExpiredFn>>,
}

impl Watcher {
    pub fn new(
        store: Arc<Store>,
        state_store: Arc<state::Store>,
        client: Arc<session::Client>,
        notify: Arc<Dispatcher>,
        log: Logger,
        selector: Option<Arc<dyn AutoSelector>>,
    ) -> Arc<Watcher> {
        Arc::new(Watcher {
            store,
            state: state_store,
            client,
            notify,
            log,
            selector,
            inner: Mutex::new(Inner {
                failures: 0,
                auth_alerted: false,
                stats: DayStats {
                    date: today(),
                    ticks: 0,
                    successes: 0,
                    pushed_new: 0,
                },
                status: Status {
                    last_verdict: "idle".to_string(),
                    last_message: "等待首次抓取".to_string(),
                    current_valid: 0,
                    pushed_today: 0,
                    failures: 0,
                    next_run: None,
                    last_success_at: String::new(),
                },
            }),
            trigger_mu: Mutex::new(false),
            trigger_cv: Condvar::new(),
            stop: AtomicBool::new(false),
            on_auth_expired: Mutex::new(None),
        })
    }

    /// Registers a callback invoked once per expiry episode.
    pub fn set_on_auth_expired(&self, f: AuthExpiredFn) {
        *self.on_auth_expired.lock().unwrap() = Some(f);
    }

    /// Asks Run to poll immediately.
    pub fn trigger_now(&self) {
        let mut g = self.trigger_mu.lock().unwrap();
        *g = true;
        self.trigger_cv.notify_all();
    }

    /// Signals the polling loop to stop. Reserved for a future graceful
    /// shutdown (the app currently exits via `process::exit`).
    #[allow(dead_code)]
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.trigger_cv.notify_all();
    }

    pub fn status(&self) -> Status {
        self.inner.lock().unwrap().status.clone()
    }

    /// Polls until stopped.
    pub fn run(self: &Arc<Self>) {
        self.log.info("监控循环启动");
        while !self.stop.load(Ordering::SeqCst) {
            self.tick();
            self.maybe_summary();

            let delay = self.next_delay();
            {
                let mut g = self.inner.lock().unwrap();
                g.status.next_run = Some(Local::now() + chrono::Duration::from_std(delay).unwrap());
            }
            self.log.debug(format!("下次抓取 {:?}", delay));

            if self.wait_for(delay) {
                self.inner.lock().unwrap().failures = 0;
            }
        }
        self.log.info("监控循环停止");
    }

    /// Waits up to `delay`; returns true if a trigger arrived.
    fn wait_for(&self, delay: Duration) -> bool {
        let start = Instant::now();
        let mut guard = self.trigger_mu.lock().unwrap();
        loop {
            if *guard {
                *guard = false;
                return true;
            }
            let remaining = delay.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return false;
            }
            let step = remaining.min(Duration::from_millis(500));
            let (g, _) = self.trigger_cv.wait_timeout(guard, step).unwrap();
            guard = g;
            if self.stop.load(Ordering::SeqCst) {
                return false;
            }
        }
    }

    pub fn tick(&self) {
        self.rollover();
        {
            let mut g = self.inner.lock().unwrap();
            g.stats.ticks += 1;
            g.status.last_verdict = "idle".to_string();
            g.status.last_message = "正在抓取…".to_string();
        }

        let probe = match site::check(&self.client) {
            Ok(p) => p,
            Err(e) => {
                self.fail_network(&e);
                return;
            }
        };

        match probe.verdict {
            site::Verdict::Courses => self.handle_courses(&probe.html),
            site::Verdict::Login => self.handle_auth_expired(&probe.reason),
            site::Verdict::Unknown => self.fail(&format!("无法判定: {}", probe.reason)),
        }
    }

    fn handle_courses(&self, page_html: &str) {
        let cfg = self.store.get();
        let result = site::parse(
            page_html,
            &site::FilterOptions {
                location_whitelist: cfg.filters.location_whitelist.clone(),
                category_blacklist: cfg.filters.category_blacklist.clone(),
            },
        );

        if !result.table_found {
            self.fail("页面可达但解析不到课程表格");
            return;
        }

        {
            let mut g = self.inner.lock().unwrap();
            g.failures = 0;
            g.auth_alerted = false;
        }
        let _ = self.state.set_auth_state(state::AuthState::Valid);

        let reconciled =
            site::reconcile_pushed(&result.courses, &self.state.get().pushed_unique_ids);
        let _ = self.state.set_pushed(&reconciled);
        let pushed_set: HashSet<String> = reconciled.into_iter().collect();

        let new_courses = site::diff_new(&result.courses, &pushed_set);
        if !new_courses.is_empty() {
            self.log.info(format!("发现新课程 {}", new_courses.len()));
            self.notify.dispatch(&notify::event::build_new_course(&new_courses));
            let _ = self.state.add_pushed(&ids_of(&new_courses));
            self.inner.lock().unwrap().stats.pushed_new += new_courses.len() as i64;
        }

        let mut select_note = String::new();
        if cfg.behavior.auto_select {
            if let Some(selector) = &self.selector {
                select_note = self.run_auto_select(selector, &result.courses);
            }
        }

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let valid = count_eligible(&result.courses);
        {
            let mut g = self.inner.lock().unwrap();
            g.stats.successes += 1;
            g.status.current_valid = valid;
            g.status.pushed_today = g.stats.pushed_new;
            g.status.failures = 0;
            g.status.last_verdict = "courses".to_string();
            g.status.last_success_at = now.clone();
            g.status.last_message = format!(
                "抓取成功 · 共 {} 条,符合条件 {} 门{}",
                result.row_count, valid, select_note
            );
            let msg = g.status.last_message.clone();
            drop(g);
            self.log.info(&msg);
        }
        let _ = self.state.update(|s| {
            s.last_run_at = now.clone();
            s.last_success_at = now.clone();
            s.consecutive_failures = 0;
        });
    }

    fn run_auto_select(&self, selector: &Arc<dyn AutoSelector>, all: &[Course]) -> String {
        let all_ids: HashSet<String> = all.iter().map(|c| c.unique_id()).collect();
        let mut handled: HashSet<String> = HashSet::new();
        let mut kept: Vec<String> = Vec::new();
        for id in &self.state.get().auto_handled_ids {
            if all_ids.contains(id) {
                kept.push(id.clone());
                handled.insert(id.clone());
            }
        }
        let _ = self.state.set_auto_handled(&kept);

        let eligible = eligible_courses(all);
        let mut targets = Vec::new();
        let mut by_id: HashMap<String, Course> = HashMap::new();
        for c in &eligible {
            by_id.insert(c.unique_id(), c.clone());
            if c.item_id.is_empty() || c.kai_ke_id.is_empty() {
                continue;
            }
            if handled.contains(&c.unique_id()) {
                continue;
            }
            targets.push(selection::Target {
                unique_id: c.unique_id(),
                item_id: c.item_id.clone(),
                kai_ke_id: c.kai_ke_id.clone(),
            });
        }
        if targets.is_empty() {
            return String::new();
        }

        self.log.info(format!("自动选课尝试 {}", targets.len()));
        let outcomes = match selector.select(&targets) {
            Ok(o) => o,
            Err(e) => {
                self.log.error(format!("自动选课异常: {}", e));
                return " · 自动选课异常".to_string();
            }
        };

        let mut selected = Vec::new();
        let mut already = Vec::new();
        let mut failed = Vec::new();
        for o in outcomes {
            match o.status {
                selection::Status::Selected => selected.push(o),
                selection::Status::Already => already.push(o),
                _ => failed.push(o),
            }
        }

        if !selected.is_empty() {
            let _ = self.state.add_auto_handled(&outcome_ids(&selected));
            self.log.info(format!("自动选课成功 {}", selected.len()));
            self.notify
                .dispatch(&notify::event::build_auto_select(&courses_of(&selected, &by_id)));
        }
        if !already.is_empty() {
            let _ = self.state.add_auto_handled(&outcome_ids(&already));
        }
        if !failed.is_empty() {
            self.log.warn(format!("自动选课失败 {}", failed.len()));
            self.notify.dispatch(&notify::event::build_auto_select_failure(
                &courses_of(&failed, &by_id),
                &outcome_messages(&failed),
            ));
        }
        format!(
            " · 自动选课 成功{}/已选{}/失败{}",
            selected.len(),
            already.len(),
            failed.len()
        )
    }

    fn handle_auth_expired(&self, reason: &str) {
        let failures = {
            let mut g = self.inner.lock().unwrap();
            g.failures += 1;
            g.status.last_verdict = "loginPage".to_string();
            g.status.failures = g.failures;
            g.status.last_message = format!("登录失效: {}", reason);
            g.failures
        };
        self.log.warn(format!("登录失效: {}", reason));
        let _ = self.state.set_auth_state(state::AuthState::Expired);
        let _ = self
            .state
            .update(|s| s.consecutive_failures = failures);

        let mut g = self.inner.lock().unwrap();
        if !g.auth_alerted {
            g.auth_alerted = true;
            drop(g);
            if let Some(f) = self.on_auth_expired.lock().unwrap().clone() {
                f(reason.to_string());
            }
        }
    }

    fn fail(&self, detail: &str) {
        let failures = {
            let mut g = self.inner.lock().unwrap();
            g.failures += 1;
            g.status.last_verdict = "error".to_string();
            g.status.failures = g.failures;
            g.status.last_message = detail.to_string();
            g.failures
        };
        self.log.warn(detail.to_string());
        let _ = self.state.update(|s| s.consecutive_failures = failures);

        let threshold = self.store.get().schedule.failure_alert_threshold as i64;
        if failures == threshold {
            self.notify
                .dispatch(&notify::event::build_runtime_error(detail));
        }
    }

    /// Records a transport-level failure (no route/DNS yet, timeout). It is
    /// transient by nature — most often the autostart instance racing the
    /// network at logon — so it retries quickly (see next_delay) and never
    /// alerts.
    fn fail_network(&self, err: &anyhow::Error) {
        let failures = {
            let mut g = self.inner.lock().unwrap();
            g.failures += 1;
            g.status.last_verdict = "network".to_string();
            g.status.failures = g.failures;
            g.status.last_message = "网络未就绪,正在重试".to_string();
            g.failures
        };
        self.log
            .warn(format!("网络暂不可用,稍后重试: {}", err));
        let _ = self.state.update(|s| s.consecutive_failures = failures);
    }

    fn maybe_summary(&self) {
        let cfg = self.store.get();
        let Some(hour) = cfg.schedule.daily_summary_hour else {
            return;
        };
        let today_str = today();
        if self.state.get().last_summary_date == today_str {
            return;
        }
        if Local::now().hour() as i32 != hour {
            return;
        }

        let (ticks, successes, pushed_new) = {
            let g = self.inner.lock().unwrap();
            (g.stats.ticks, g.stats.successes, g.stats.pushed_new)
        };
        let current_valid = self.inner.lock().unwrap().status.current_valid;
        let auth = self.state.get().auth_state.as_str().to_string();
        self.notify
            .dispatch(&notify::event::build_daily_summary(&notify::event::SummaryStats {
                date: today_str.clone(),
                ticks,
                successes,
                pushed_new,
                current_valid_count: current_valid,
                auth_state: auth,
                tracked_count: self.state.get().pushed_unique_ids.len(),
            }));
        let _ = self.state.update(|s| s.last_summary_date = today_str);
    }

    fn next_delay(&self) -> Duration {
        let (last_verdict, failures) = {
            let g = self.inner.lock().unwrap();
            (g.status.last_verdict.clone(), g.failures)
        };
        if last_verdict == "network" {
            let secs = (20 + (failures - 1).max(0) * 15).clamp(1, 120) as u64;
            return Duration::from_secs(secs);
        }
        let cfg = self.store.get().schedule;
        let mut base = cfg.refresh_interval_ms as f64;
        if failures > 0 {
            base = (base * 2f64.powi(failures as i32)).min(cfg.max_backoff_ms as f64);
        }
        let jitter = 1.0 + (rand::random::<f64>() * 2.0 - 1.0) * cfg.jitter_ratio;
        let delay = Duration::from_secs_f64(base * jitter / 1000.0);
        delay.max(Duration::from_secs(15))
    }

    fn rollover(&self) {
        let d = today();
        let mut g = self.inner.lock().unwrap();
        if d != g.stats.date {
            g.stats = DayStats {
                date: d,
                ticks: 0,
                successes: 0,
                pushed_new: 0,
            };
        }
    }
}

fn outcome_ids(outcomes: &[selection::Outcome]) -> Vec<String> {
    outcomes.iter().map(|o| o.target.unique_id.clone()).collect()
}

fn outcome_messages(outcomes: &[selection::Outcome]) -> Vec<String> {
    outcomes.iter().map(|o| o.message.clone()).collect()
}

fn courses_of(outcomes: &[selection::Outcome], by_id: &HashMap<String, Course>) -> Vec<Course> {
    outcomes
        .iter()
        .filter_map(|o| by_id.get(&o.target.unique_id).cloned())
        .collect()
}

fn eligible_courses(courses: &[Course]) -> Vec<Course> {
    courses.iter().filter(|c| c.eligible()).cloned().collect()
}

fn count_eligible(courses: &[Course]) -> usize {
    courses.iter().filter(|c| c.eligible()).count()
}

fn ids_of(courses: &[Course]) -> Vec<String> {
    courses.iter().map(|c| c.unique_id()).collect()
}

fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}
