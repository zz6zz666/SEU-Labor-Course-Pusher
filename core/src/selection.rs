//! Submits course selections (选课) and cancellations (退选) over plain HTTP,
//! without any browser.
//!
//! The course page is server-rendered, so an authenticated GET yields
//! everything needed: the antiforgery token (a hidden input) and each row's
//! IDs. The site's own changeAjax.postAntiForgery is a plain form POST carrying
//! that token, so a bare request is equivalent to clicking the button.

use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, Result};
use regex::Regex;

use crate::session;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Selected,
    /// Reserved: produced by [`Runner::cancel`], kept for a future automatic
    /// cancellation feature.
    #[allow(dead_code)]
    Cancelled,
    Already,
    NotFound,
    Failed,
}

/// Identifies a course row by its stable (SJItemID, SJItemKaiKeID) pair.
#[derive(Clone)]
pub struct Target {
    pub unique_id: String,
    pub item_id: String,
    pub kai_ke_id: String,
}

pub struct Outcome {
    pub target: Target,
    pub status: Status,
    pub message: String,
}

/// Submits selections for eligible courses (over HTTP).
pub trait AutoSelector: Send + Sync {
    fn select(&self, targets: &[Target]) -> Result<Vec<Outcome>>;
}

const SELECT_PATH: &str = "/SJItemKaiKe/XuanKe/StudentXuanKe";
/// Reserved for the future automatic-cancellation feature.
#[allow(dead_code)]
const CANCEL_PATH: &str = "/SJItemKaiKe/XuanKe/StudentCancelXuanKe";

fn re_token() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"name="__RequestVerificationToken"[^>]*value="([^"]+)""#).unwrap()
    })
}
fn re_row() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<tr[^>]*c--tr.*?</tr>").unwrap())
}
/// Reserved for the future automatic-cancellation feature.
#[allow(dead_code)]
fn re_id() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"data-name="ID"[^>]*data-value="([^"]*)""#).unwrap())
}

pub struct Runner {
    pub client: Arc<session::Client>,
}

impl Runner {
    /// Submits course selections.
    pub fn select(&self, targets: &[Target]) -> Result<Vec<Outcome>> {
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let (html, token) = self.course_page()?;

        let mut out = Vec::with_capacity(targets.len());
        for t in targets {
            let row = row_for(&html, t);
            if row.is_empty() {
                out.push(Outcome {
                    target: t.clone(),
                    status: Status::NotFound,
                    message: "页面上找不到对应行".to_string(),
                });
                continue;
            }
            let button = button_tag(&row, "StudentXuanKe");
            if button.is_empty() {
                out.push(Outcome {
                    target: t.clone(),
                    status: Status::NotFound,
                    message: "找不到选课按钮".to_string(),
                });
                continue;
            }
            if disabled(&button) {
                out.push(Outcome {
                    target: t.clone(),
                    status: Status::Already,
                    message: "按钮不可用(已选或已满)".to_string(),
                });
                continue;
            }
            let res = self.post(
                SELECT_PATH,
                &[
                    ("SJItemID", t.item_id.as_str()),
                    ("SJItemKaiKeID", t.kai_ke_id.as_str()),
                    ("__RequestVerificationToken", token.as_str()),
                ],
            );
            out.push(outcome_of(t, res, Status::Selected));
        }
        Ok(out)
    }

    /// Withdraws course selections. Unlike selection it also needs the row's
    /// record ID, taken from the same page.
    ///
    /// Reserved: not wired to the UI yet, but kept for a future automatic
    /// cancellation feature (e.g. cancel a course the user has not responded
    /// about within some time).
    #[allow(dead_code)]
    pub fn cancel(&self, targets: &[Target]) -> Result<Vec<Outcome>> {
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let (html, token) = self.course_page()?;

        let mut out = Vec::with_capacity(targets.len());
        for t in targets {
            let row = row_for(&html, t);
            if row.is_empty() {
                out.push(Outcome {
                    target: t.clone(),
                    status: Status::NotFound,
                    message: "页面上找不到对应行".to_string(),
                });
                continue;
            }
            let button = button_tag(&row, "StudentCancelXuanKe");
            if button.is_empty() {
                out.push(Outcome {
                    target: t.clone(),
                    status: Status::NotFound,
                    message: "找不到取消选课按钮(可能未选该课)".to_string(),
                });
                continue;
            }
            if disabled(&button) {
                out.push(Outcome {
                    target: t.clone(),
                    status: Status::Already,
                    message: "取消选课按钮不可用".to_string(),
                });
                continue;
            }
            let id = re_id()
                .captures(&row)
                .map(|c| c[1].to_string())
                .unwrap_or_default();
            if id.is_empty() {
                out.push(Outcome {
                    target: t.clone(),
                    status: Status::Failed,
                    message: "缺少选课记录 ID".to_string(),
                });
                continue;
            }
            let res = self.post(
                CANCEL_PATH,
                &[
                    ("ID", id.as_str()),
                    ("SJItemID", t.item_id.as_str()),
                    ("SJItemKaiKeID", t.kai_ke_id.as_str()),
                    ("__RequestVerificationToken", token.as_str()),
                ],
            );
            out.push(outcome_of(t, res, Status::Cancelled));
        }
        Ok(out)
    }

    fn course_page(&self) -> Result<(String, String)> {
        let resp = self.client.get(session::COURSE_PAGE)?;
        if resp.status != 200 {
            return Err(anyhow!("选课页返回 {}", resp.status));
        }
        let token = match re_token().captures(&resp.body) {
            Some(caps) => caps[1].to_string(),
            None => return Err(anyhow!("选课页缺少请求令牌(登录态可能已失效)")),
        };
        Ok((resp.body, token))
    }

    fn post(&self, path: &str, form: &[(&str, &str)]) -> PostResult {
        let url = site_url(path);
        let resp = match self.client.post_form(&url, form) {
            Ok(r) => r,
            Err(e) => return PostResult::Err(e.to_string()),
        };
        #[derive(serde::Deserialize)]
        struct Res {
            #[serde(rename = "Success", default)]
            success: bool,
            #[serde(rename = "Message", default)]
            message: String,
        }
        match serde_json::from_str::<Res>(&resp.body) {
            Ok(r) => PostResult::Ok(r.success, r.message),
            Err(_) => PostResult::Err(format!("响应不是 JSON: {}", first_line(&resp.body))),
        }
    }
}

enum PostResult {
    Ok(bool, String),
    Err(String),
}

fn outcome_of(t: &Target, res: PostResult, success: Status) -> Outcome {
    match res {
        PostResult::Err(msg) => Outcome {
            target: t.clone(),
            status: Status::Failed,
            message: msg,
        },
        PostResult::Ok(true, msg) => Outcome {
            target: t.clone(),
            status: success,
            message: msg,
        },
        PostResult::Ok(false, msg) => Outcome {
            target: t.clone(),
            status: Status::Failed,
            message: msg,
        },
    }
}

fn row_for(html: &str, t: &Target) -> String {
    let item = format!(r#"data-name="SJItemID" data-value="{}""#, t.item_id);
    let kai = format!(r#"data-name="SJItemKaiKeID" data-value="{}""#, t.kai_ke_id);
    for row in re_row().find_iter(html) {
        let row = row.as_str();
        if row.contains(&item) && row.contains(&kai) {
            return row.to_string();
        }
    }
    String::new()
}

/// Returns the opening tag of the button with the given data-command.
fn button_tag(row: &str, command: &str) -> String {
    let marker = format!(r#"data-command="{}""#, command);
    let Some(i) = row.find(&marker) else {
        return String::new();
    };
    let Some(start) = row[..i].rfind('<') else {
        return String::new();
    };
    let Some(end) = row[i..].find('>') else {
        return String::new();
    };
    row[start..i + end + 1].to_string()
}

fn disabled(tag: &str) -> bool {
    tag.contains("disabled") || tag.contains("c--lock")
}

fn site_url(path: &str) -> String {
    match url::Url::parse(session::COURSE_PAGE) {
        Ok(u) => format!("{}://{}{}", u.scheme(), u.host_str().unwrap_or(""), path),
        Err(_) => path.to_string(),
    }
}

fn first_line(s: &str) -> String {
    let s = s.split(['\r', '\n']).next().unwrap_or("");
    let s = if s.len() > 200 { &s[..200] } else { s };
    s.to_string()
}

impl AutoSelector for Runner {
    fn select(&self, targets: &[Target]) -> Result<Vec<Outcome>> {
        Runner::select(self, targets)
    }
}
