//! Embeds the icon and version resource into the executable, so it looks
//! identical in Explorer and the file-properties dialog. The shared helper
//! skips non-Windows/MSVC targets, so `cargo check` on CI stays green.

fn main() {
    winres_embed::embed(winres_embed::Resources {
        icon: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("icon.ico"),
        version: winres_embed::Version::Env {
            var: "SEU_LABOR_VERSION".to_string(),
            fallback: env!("CARGO_PKG_VERSION").to_string(),
        },
        company: "SEU".to_string(),
        file_description: "SEU 劳动教育课程推送助手".to_string(),
        internal_name: "seu-labor".to_string(),
        original_filename: "seu-labor.exe".to_string(),
        product_name: "SEU 劳动教育课程推送助手".to_string(),
        copyright: "MIT License".to_string(),
        comments: "https://github.com/zz6zz666/SEU-Labor-Course-Pusher".to_string(),
    });
}
