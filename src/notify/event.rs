//! Event formatting for notification channels (PushPlus markdown, desktop
//! toast).

use crate::site::Course;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    NewCourse,
    AuthExpired,
    RuntimeError,
    DailySummary,
    AutoSelect,
}

impl EventType {
    pub fn as_str(self) -> &'static str {
        match self {
            EventType::NewCourse => "newCourse",
            EventType::AuthExpired => "authExpired",
            EventType::RuntimeError => "runtimeError",
            EventType::DailySummary => "dailySummary",
            EventType::AutoSelect => "autoSelect",
        }
    }
}

pub struct Event {
    pub kind: EventType,
    pub title: String,
    pub markdown: String,
    pub body: String,
}

fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn format_courses_markdown(courses: &[Course], title: &str) -> String {
    let mut b = String::new();
    if !title.is_empty() {
        b.push_str(&format!("## {}\n\n", title));
    }
    b.push_str("| 序号 | 项目名称 | 项目类别 | 实施时间 | 开课地点 | 选课情况 | 教师 |\n");
    b.push_str("|------|----------|----------|----------|----------|----------|------|\n");
    for c in courses {
        b.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            c.seq, c.name, c.category, c.time, c.location, c.enrollment, c.teacher
        ));
    }
    b.push_str(&format!("\n提取时间：{}", timestamp()));
    b
}

fn course_preview(courses: &[Course]) -> String {
    let mut b = String::new();
    for (i, c) in courses.iter().enumerate() {
        if i == 3 {
            b.push_str(&format!("\n…另有 {} 门", courses.len() - 3));
            break;
        }
        if i > 0 {
            b.push('\n');
        }
        b.push_str(&format!("· {}｜{}｜{}", c.name, c.time, c.location));
    }
    b
}

pub fn build_new_course(courses: &[Course]) -> Event {
    Event {
        kind: EventType::NewCourse,
        title: format!("劳动教育新课程 · {} 门", courses.len()),
        markdown: format_courses_markdown(courses, &format!("发现 {} 门新课程", courses.len())),
        body: format!("{}\n\n点击在内置浏览器中打开选课页", course_preview(courses)),
    }
}

pub fn build_auto_select(courses: &[Course]) -> Event {
    Event {
        kind: EventType::AutoSelect,
        title: format!("已自动选课 · {} 门", courses.len()),
        markdown: format_courses_markdown(courses, &format!("已自动选课 {} 门", courses.len())),
        body: format!(
            "{}\n\n自动选课已提交。点击在内置浏览器中打开选课页核对。",
            course_preview(courses)
        ),
    }
}

pub fn build_auto_select_failure(courses: &[Course], reasons: &[String]) -> Event {
    let mut detail = String::new();
    for (i, c) in courses.iter().enumerate() {
        if i == 3 {
            break;
        }
        if i > 0 {
            detail.push('\n');
        }
        let reason = if i < reasons.len() && !reasons[i].is_empty() {
            reasons[i].as_str()
        } else {
            "未知原因"
        };
        detail.push_str(&format!("· {}｜{}：{}", c.name, c.time, reason));
    }
    Event {
        kind: EventType::AutoSelect,
        title: format!("自动选课失败 · {} 门", courses.len()),
        markdown: format!(
            "{}\n\n**失败原因**：\n{}\n\n程序会在后续每轮继续重试,直到成功或课程失效。",
            format_courses_markdown(courses, &format!("自动选课失败 {} 门", courses.len())),
            detail
        ),
        body: format!("{}\n\n程序会继续重试。点击在内核浏览器中打开选课页", detail),
    }
}

/// Sent only after unattended re-login has failed, so it really does require a
/// human.
pub fn build_auth_expired(reason: &str) -> Event {
    Event {
        kind: EventType::AuthExpired,
        title: "登录失效 · 需要手动登录".to_string(),
        markdown: format!(
            "## 统一身份认证登录已失效\n\n**原因**：{}\n\n自动重新登录未能完成(可能需要短信/图形验证码)。\n\n**时间**：{}\n\n点击本机通知将打开登录页。",
            reason,
            timestamp()
        ),
        body: format!("自动登录失败({}),点击打开登录页", reason),
    }
}

pub fn build_runtime_error(detail: &str) -> Event {
    Event {
        kind: EventType::RuntimeError,
        title: "监控运行异常".to_string(),
        markdown: format!(
            "## 监控运行异常\n\n**详情**：{}\n\n**时间**：{}\n\n已连续失败若干次,程序会自动退避重试。",
            detail,
            timestamp()
        ),
        body: format!("{}\n点击打开日志目录", detail),
    }
}

pub struct SummaryStats {
    pub date: String,
    pub ticks: i64,
    pub successes: i64,
    pub pushed_new: i64,
    pub current_valid_count: usize,
    pub auth_state: String,
    pub tracked_count: usize,
}

pub fn build_daily_summary(s: &SummaryStats) -> Event {
    let auth_text = match s.auth_state.as_str() {
        "valid" => "正常",
        "expired" => "已失效",
        _ => "未知",
    };
    Event {
        kind: EventType::DailySummary,
        title: format!("运行汇总 · {}", s.date),
        markdown: format!(
            "## 每日运行汇总({})\n\n| 指标 | 数值 |\n|---|---|\n| 抓取次数 | {} |\n| 成功次数 | {} |\n| 新课程推送 | {} |\n| 当前可选课程 | {} |\n| 登录态 | {} |\n| 已记录课程 | {} |\n\n统计时间：{}",
            s.date,
            s.ticks,
            s.successes,
            s.pushed_new,
            s.current_valid_count,
            auth_text,
            s.tracked_count,
            timestamp()
        ),
        body: format!(
            "抓取 {} 次 / 成功 {} 次\n新课程推送 {} 门,当前可选 {} 门\n登录态:{}",
            s.ticks, s.successes, s.pushed_new, s.current_valid_count, auth_text
        ),
    }
}
