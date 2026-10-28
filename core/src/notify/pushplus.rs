//! PushPlus channel: WeChat notifications via pushplus.plus.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::config::Store;
use crate::logging::Logger;
use crate::session;

use super::{Channel, Event};

pub struct PushPlus {
    store: Arc<Store>,
    log: Logger,
    agent: ureq::Agent,
}

impl PushPlus {
    pub fn new(store: Arc<Store>, log: Logger) -> PushPlus {
        let agent = crate::session::build_agent(Duration::from_secs(15));
        PushPlus { store, log, agent }
    }
}

impl Channel for PushPlus {
    fn name(&self) -> &'static str {
        "pushplus"
    }

    fn send(&self, e: &Event) -> Result<()> {
        let cfg = self.store.get().push.pushplus;
        if !cfg.enabled {
            return Ok(());
        }
        if cfg.token.is_empty() {
            return Err(anyhow!("未配置 PushPlus Token"));
        }

        let payload = serde_json::json!({
            "token": cfg.token,
            "title": e.title,
            "content": e.markdown,
            "template": "markdown",
        })
        .to_string();

        let resp = self
            .agent
            .post(session::PUSHPLUS)
            .set("Content-Type", "application/json; charset=utf-8")
            .send_string(&payload)
            .map_err(|err| anyhow!("{}", err))?;
        let body = resp.into_string()?;
        let parsed: serde_json::Value =
            serde_json::from_str(&body).map_err(|err| anyhow!("PushPlus 响应解析失败: {}", err))?;
        let code = parsed.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
        if code != 200 {
            let msg = parsed.get("msg").and_then(|v| v.as_str()).unwrap_or("");
            return Err(anyhow!("PushPlus 拒绝: code={} msg={}", code, msg));
        }
        self.log.info(format!("微信推送成功 {}", e.kind.as_str()));
        Ok(())
    }
}
