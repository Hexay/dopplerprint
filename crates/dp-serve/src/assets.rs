//! The page, embedded at compile time. No build step and no third-party resources.

pub const APP_CSP: &str =
    "default-src 'self'; img-src 'self' data:; frame-src 'self'; base-uri 'none'; form-action 'none'";

const JS: &str = "text/javascript; charset=utf-8";

/// Body and content type of a static file by its URL path (without the leading slash).
pub fn get(path: &str) -> Option<(&'static str, &'static str)> {
    Some(match path {
        "" => (include_str!("web/index.html"), "text/html; charset=utf-8"),
        "style.css" => (include_str!("web/style.css"), "text/css; charset=utf-8"),
        "app.js" => (include_str!("web/app.js"), JS),
        "job.js" => (include_str!("web/job.js"), JS),
        "when.js" => (include_str!("web/when.js"), JS),
        _ => return None,
    })
}
