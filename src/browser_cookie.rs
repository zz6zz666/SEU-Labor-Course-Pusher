//! Adapters between the desktop browser engine's cookie shape and the core's
//! persisted shape.

use seu_labor_core::session::jar::StoredCookie;

pub fn to_stored(c: browserhost::Cookie) -> StoredCookie {
    StoredCookie {
        name: c.name,
        value: c.value,
        domain: c.domain,
        path: c.path,
        expires: c.expires,
        secure: c.secure,
        http_only: c.http_only,
    }
}

pub fn to_browser(c: &StoredCookie) -> browserhost::Cookie {
    browserhost::Cookie {
        name: c.name.clone(),
        value: c.value.clone(),
        domain: c.domain.clone(),
        path: c.path.clone(),
        expires: c.expires,
        secure: c.secure,
        http_only: c.http_only,
    }
}
