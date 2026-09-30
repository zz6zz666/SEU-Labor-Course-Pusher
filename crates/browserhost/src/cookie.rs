//! A browser cookie in the shape CDP reports and accepts. Deliberately free of
//! any host-specific cookie model so the crate stays reusable.

#[derive(Clone, Debug, Default)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    /// Unix seconds, or `0` for a session cookie.
    pub expires: i64,
    pub secure: bool,
    pub http_only: bool,
}
