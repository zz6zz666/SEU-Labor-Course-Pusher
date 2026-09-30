//! Turns the SEU labor course page into structured data.
//!
//! The list is server-rendered (verified against a live session): a plain HTTP
//! GET returns the table with data rows, so no browser engine is needed to
//! poll.

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::{Datelike, NaiveDate};
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::session;

pub const COURSE_TABLE_ID: &str = "c_app_page_index_XuanKe_table";

#[derive(Clone)]
pub struct Course {
    pub seq: String,
    pub name: String,
    pub category: String,
    pub location: String,
    pub time: String,
    pub enrollment: String,
    pub teacher: String,
    pub invalid: bool,
    pub location_ok: bool,
    pub category_ok: bool,
    pub item_id: String,
    pub kai_ke_id: String,
}

impl Course {
    pub fn unique_id(&self) -> String {
        format!("{}|{}", self.name, self.time)
    }
    pub fn eligible(&self) -> bool {
        self.location_ok && self.category_ok && !self.invalid
    }
}

/// Mirrors the config semantics: locations is a substring whitelist, categories
/// is an exact-match blacklist.
#[derive(Clone, Default)]
pub struct FilterOptions {
    pub locations: Vec<String>,
    pub categories: Vec<String>,
}

impl FilterOptions {
    fn match_location(&self, loc: &str) -> bool {
        if self.locations.is_empty() {
            return true;
        }
        self.locations.iter().any(|kw| loc.contains(kw))
    }
    fn match_category(&self, cat: &str) -> bool {
        !self.categories.iter().any(|black| cat == black)
    }
}

#[derive(Default)]
pub struct ParseResult {
    pub table_found: bool,
    pub row_count: usize,
    pub courses: Vec<Course>,
}

/// DiffNew returns eligible courses whose unique id is not already tracked.
pub fn diff_new(courses: &[Course], pushed: &std::collections::HashSet<String>) -> Vec<Course> {
    courses
        .iter()
        .filter(|c| c.eligible() && !pushed.contains(&c.unique_id()))
        .cloned()
        .collect()
}

/// ReconcilePushed drops tracked ids that disappeared, became invalid or no
/// longer match the filters.
pub fn reconcile_pushed(courses: &[Course], pushed: &[String]) -> Vec<String> {
    let by_id: HashMap<String, &Course> =
        courses.iter().map(|c| (c.unique_id(), c)).collect();
    pushed
        .iter()
        .filter(|id| by_id.get(*id).map(|c| c.eligible()).unwrap_or(false))
        .cloned()
        .collect()
}

// --------------------------------------------------------------------- parse

fn sel_table() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| Selector::parse(&format!("table#{}", COURSE_TABLE_ID)).unwrap())
}
fn sel_tbody() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| Selector::parse("tbody").unwrap())
}
fn sel_limit_line() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| Selector::parse(".limit-line").unwrap())
}
fn sel_td_data() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| Selector::parse("td-data").unwrap())
}

pub fn parse(page_html: &str, opts: &FilterOptions) -> ParseResult {
    let doc = Html::parse_document(page_html);
    let Some(table) = doc.select(sel_table()).next() else {
        return ParseResult::default();
    };

    let rows = course_rows(table);
    let mut result = ParseResult {
        table_found: true,
        row_count: rows.len(),
        courses: Vec::with_capacity(rows.len()),
    };
    for row in rows {
        let cells = child_elements(row, "td");
        if cells.len() < 4 {
            continue;
        }
        result.courses.push(build_course(&cells, opts));
    }
    result
}

fn course_rows(table: ElementRef<'_>) -> Vec<ElementRef<'_>> {
    let Some(tbody) = table.select(sel_tbody()).next() else {
        return Vec::new();
    };
    child_elements(tbody, "tr")
        .into_iter()
        .filter(|tr| {
            tr.value()
                .classes()
                .any(|c| c == "c--tr" || c == "c-tr")
        })
        .collect()
}

fn child_elements<'a>(el: ElementRef<'a>, tag: &str) -> Vec<ElementRef<'a>> {
    el.children()
        .filter_map(ElementRef::wrap)
        .filter(|e| e.value().name() == tag)
        .collect()
}

fn build_course(cells: &[ElementRef<'_>], opts: &FilterOptions) -> Course {
    let cell_text = |n: usize| -> String {
        if n < 1 || n > cells.len() {
            return "无".to_string();
        }
        clean_text(&text_of(cells[n - 1]))
    };

    let col1 = cell_text(1);
    let col2 = cell_text(2);
    let offset = if is_pure_number(&col1) && !is_pure_number(&col2) {
        0
    } else {
        1
    };

    let name = cell_text(3 + offset);
    let time = cell_text(8 + offset);
    let deadline = cell_text(9 + offset);
    let enrollment = cell_text(10 + offset);

    let location = if 7 + offset <= cells.len() {
        cells[7 + offset - 1]
            .select(sel_limit_line())
            .next()
            .map(|l| clean_text(&text_of(l)))
            .unwrap_or_else(|| "无".to_string())
    } else {
        "无".to_string()
    };

    let seq = if offset == 1 { col2 } else { col1 };
    let full = enrollment.contains("已满");
    let expired = deadline.contains("已截止");
    let td = td_data_map(cells[0]);
    let category = cell_text(4 + offset);

    Course {
        seq,
        name,
        category: category.clone(),
        location: location.clone(),
        time: append_weekday(&time),
        enrollment,
        teacher: cell_text(15 + offset),
        invalid: full || expired,
        location_ok: opts.match_location(&location),
        category_ok: opts.match_category(&category),
        item_id: td.get("SJItemID").cloned().unwrap_or_default(),
        kai_ke_id: td.get("SJItemKaiKeID").cloned().unwrap_or_default(),
    }
}

fn td_data_map(cell: ElementRef<'_>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for n in cell.select(sel_td_data()) {
        if let Some(name) = n.value().attr("data-name") {
            if !name.is_empty() {
                out.insert(
                    name.to_string(),
                    n.value().attr("data-value").unwrap_or("").to_string(),
                );
            }
        }
    }
    out
}

fn text_of(el: ElementRef<'_>) -> String {
    el.text().collect::<String>()
}

fn clean_text(s: &str) -> String {
    let s = s.replace('\u{a0}', " ");
    let s = whitespace().replace_all(&s, " ");
    let s = s.trim().to_string();
    if s.is_empty() {
        "无".to_string()
    } else {
        s
    }
}

fn append_weekday(s: &str) -> String {
    let Some(m) = date_regex().find(s) else {
        return s.to_string();
    };
    let Ok(d) = NaiveDate::parse_from_str(m.as_str(), "%Y-%m-%d") else {
        return s.to_string();
    };
    const WEEKDAYS: [&str; 7] = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    format!("{}（{}）", s, WEEKDAYS[d.weekday().num_days_from_sunday() as usize])
}

fn whitespace() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").unwrap())
}
fn date_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap())
}
fn pure_number() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d+$").unwrap())
}

fn is_pure_number(s: &str) -> bool {
    pure_number().is_match(s.trim())
}

// --------------------------------------------------------------------- probe

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Unknown,
    Courses,
    Login,
}

/// Answers the login question by asking the server, not by guessing from
/// heartbeats: a course table means a valid session, a redirect to the auth
/// host means it expired.
pub struct Probe {
    pub verdict: Verdict,
    pub reason: String,
    pub html: String,
}

const LOGIN_MARKERS: [&str; 7] = [
    "casLogin",
    "needCaptcha",
    "统一身份认证",
    "CaptchaDeText",
    "CaptchaInputText",
    "input-username-pc",
    "AuthServer/Login",
];

pub fn check(client: &session::Client) -> anyhow::Result<Probe> {
    let res = client.get(session::COURSE_PAGE)?;
    if res.body.contains(COURSE_TABLE_ID) {
        return Ok(Probe {
            verdict: Verdict::Courses,
            reason: "返回选课表格".to_string(),
            html: res.body,
        });
    }
    if redirected_to_auth(&res.final_url) {
        return Ok(Probe {
            verdict: Verdict::Login,
            reason: "被重定向到统一身份认证".to_string(),
            html: res.body,
        });
    }
    if LOGIN_MARKERS.iter().any(|m| res.body.contains(m)) {
        return Ok(Probe {
            verdict: Verdict::Login,
            reason: "正文含登录页特征".to_string(),
            html: res.body,
        });
    }
    Ok(Probe {
        verdict: Verdict::Unknown,
        reason: format!("无法判定({})", res.final_url),
        html: res.body,
    })
}

fn redirected_to_auth(final_url: &str) -> bool {
    let Ok(u) = url::Url::parse(final_url) else {
        return false;
    };
    u.host_str() == Some("auth.seu.edu.cn") || u.path().contains("AuthServer/Login")
}
