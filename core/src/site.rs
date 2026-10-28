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

/// Mirrors the config semantics: `location_whitelist` is a substring whitelist,
/// `category_blacklist` is an exact-match blacklist.
#[derive(Clone, Default)]
pub struct FilterOptions {
    pub location_whitelist: Vec<String>,
    pub category_blacklist: Vec<String>,
}

impl FilterOptions {
    fn match_location(&self, loc: &str) -> bool {
        if self.location_whitelist.is_empty() {
            return true;
        }
        self.location_whitelist.iter().any(|kw| loc.contains(kw))
    }
    fn match_category(&self, cat: &str) -> bool {
        !self.category_blacklist.iter().any(|black| cat == black)
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn page(enrollment: &str) -> String {
        format!(
            r#"<!doctype html><html><body>
<table id="{table}"><tbody>
<tr class="c--tr">
<td><td-data data-name="SJItemID" data-value="item-1"></td-data><td-data data-name="SJItemKaiKeID" data-value="kaike-1"></td-data></td>
<td>1</td>
<td>f2</td>
<td>课程A</td>
<td>类别X</td>
<td>f5</td>
<td>f6</td>
<td><div class="limit-line">九龙湖</div></td>
<td>2026-11-01 08:00-10:00</td>
<td>2026-10-30 12:00</td>
<td>{enrollment}</td>
<td>f11</td>
<td>f12</td>
<td>f13</td>
<td>f14</td>
<td>张老师</td>
</tr>
</tbody></table></body></html>"#,
            table = COURSE_TABLE_ID,
            enrollment = enrollment,
        )
    }

    fn eligible_filters() -> FilterOptions {
        FilterOptions {
            location_whitelist: vec!["九龙湖".to_string()],
            category_blacklist: vec!["讲座".to_string()],
        }
    }

    #[test]
    fn parses_one_eligible_row() {
        let result = parse(&page("已选 1/10"), &eligible_filters());
        assert!(result.table_found);
        assert_eq!(result.row_count, 1);
        let course = &result.courses[0];
        assert_eq!(course.seq, "1");
        assert_eq!(course.name, "课程A");
        assert_eq!(course.category, "类别X");
        assert_eq!(course.location, "九龙湖");
        assert!(course.time.starts_with("2026-11-01 08:00-10:00"));
        assert!(course.time.contains("（周"));
        assert_eq!(course.enrollment, "已选 1/10");
        assert_eq!(course.teacher, "张老师");
        assert_eq!(course.item_id, "item-1");
        assert_eq!(course.kai_ke_id, "kaike-1");
        assert!(course.eligible());
    }

    #[test]
    fn location_whitelist_and_category_blacklist_filter_rows() {
        let wrong_location = FilterOptions {
            location_whitelist: vec!["四牌楼".to_string()],
            category_blacklist: Vec::new(),
        };
        let result = parse(&page("已选 1/10"), &wrong_location);
        assert!(!result.courses[0].location_ok);
        assert!(!result.courses[0].eligible());

        let blacklisted = FilterOptions {
            location_whitelist: Vec::new(),
            category_blacklist: vec!["类别X".to_string()],
        };
        let result = parse(&page("已选 1/10"), &blacklisted);
        assert!(!result.courses[0].category_ok);
        assert!(!result.courses[0].eligible());
    }

    #[test]
    fn full_enrollment_is_invalid() {
        let result = parse(&page("已满"), &eligible_filters());
        assert!(result.courses[0].invalid);
        assert!(!result.courses[0].eligible());
    }

    #[test]
    fn diff_and_reconcile_use_name_and_time() {
        let result = parse(&page("已选 1/10"), &eligible_filters());
        let course = &result.courses[0];

        let new = diff_new(&result.courses, &HashSet::new());
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].unique_id(), course.unique_id());

        let mut pushed = HashSet::new();
        pushed.insert(course.unique_id());
        assert!(diff_new(&result.courses, &pushed).is_empty());

        let kept = reconcile_pushed(&result.courses, &[course.unique_id()]);
        assert_eq!(kept, vec![course.unique_id()]);
    }

    #[test]
    fn a_missing_table_is_reported() {
        let result = parse("<html><body>登录页</body></html>", &eligible_filters());
        assert!(!result.table_found);
        assert_eq!(result.row_count, 0);
        assert!(result.courses.is_empty());
    }
}
