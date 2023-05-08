//! [`Catcher`] is the default implement [`Handler`] for catch page error.
//!
//! A web application can specify several different Catchers to handle errors.
//!
//! They can be set via the `with_catchers` function of `Server`:
//!
//! # Example
//!
//! ```
//! # use salvo_core::prelude::*;
//! # use salvo_core::catcher::Catcher;
//!
//! #[handler]
//! async fn handle404(&self, _req: &Request, _depot: &Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
//!     if let Some(StatusCode::NOT_FOUND) = res.status_code {
//!         res.render("Custom 404 Error Page");
//!         ctrl.skip_rest();
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     Service::new(Router::new()).catcher(Catcher::default().hoop(handle404));
//! }
//! ```
//!
//! When there is an error in the website request result, first try to set the error page
//! through the [`Catcher`] set by the user. If the [`Catcher`] catches the error,
//! it will return `true`.
//!
//! If your custom catchers does not capture this error, then the system uses
//! [`write_error_default`] to capture processing errors and send the default error page.

use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use mime::Mime;
use once_cell::sync::Lazy;

use crate::handler::{Handler, WhenHoop};
use crate::http::{guess_accept_mime, ResBody, header, Request, Response, StatusCode, StatusError};
use crate::{Depot, FlowCtrl};

static SUPPORTED_FORMATS: Lazy<Vec<mime::Name>> = Lazy::new(|| vec![mime::JSON, mime::HTML, mime::XML, mime::PLAIN]);
const EMPTY_DETAIL_MSG: &str = "there is no more detailed explanation";
const SALVO_LINK: &str = r#"<a href="https://salvo.rs" target="_blank">salvo</a>"#;

#[inline]
fn status_error_html(
    code: StatusCode,
    name: &str,
    summary: Option<&str>,
    detail: Option<&str>,
    footer: Option<&str>,
) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width">
    <title>{0}: {1}</title>
    <style>
    :root {{
        --bg-color: #fff;
        --text-color: #222;
    }}
    body {{
        background: var(--bg-color);
        color: var(--text-color);
        text-align: center;
    }}
    footer{{text-align:center;}}
    @media (prefers-color-scheme: dark) {{
        :root {{
            --bg-color: #222;
            --text-color: #ddd;
        }}
        a:link {{ color: red; }}
        a:visited {{ color: #a8aeff; }}
        a:hover {{color: #a8aeff;}}
        a:active {{color: #a8aeff;}}
    }}
    </style>
</head>
<body>
    <div><h1>{0}: {1}</h1>{2}{3}<hr><footer>{4}</footer></div>
</body>
</html>"#,
        code.as_u16(),
        name,
        summary.map(|summary| format!("<h3>{summary}</h3>")).unwrap_or_default(),
        format_args!("<p>{}</p>", detail.unwrap_or(EMPTY_DETAIL_MSG)),
        footer.unwrap_or(SALVO_LINK)
    )
}
#[inline]
fn status_error_json(code: StatusCode, name: &str, summary: Option<&str>, detail: Option<&str>) -> String {
    format!(
        r#"{{"error":{{"code":{},"name":"{}","summary":"{}","detail":"{}"}}}}"#,
        code.as_u16(),
        name,
        summary.unwrap_or(name),
        detail.unwrap_or(EMPTY_DETAIL_MSG)
    )
}
#[inline]
fn status_error_plain(code: StatusCode, name: &str, summary: Option<&str>, detail: Option<&str>) -> String {
    format!(
        "code:{},\nname:{},\nsummary:{},\ndetail:{}",
        code.as_u16(),
        name,
        summary.unwrap_or(name),
        detail.unwrap_or(EMPTY_DETAIL_MSG)
    )
}
#[inline]
fn status_error_xml(code: StatusCode, name: &str, summary: Option<&str>, detail: Option<&str>) -> String {
    format!(
        "<error><code>{}</code><name>{}</name><summary>{}</summary><detail>{}</detail></error>",
        code.as_u16(),
        name,
        summary.unwrap_or(name),
        detail.unwrap_or(EMPTY_DETAIL_MSG)
    )
}
/// Create bytes from `StatusError`.
#[inline]
pub fn status_error_bytes(err: &StatusError, prefer_format: &Mime, footer: Option<&str>) -> (Mime, Vec<u8>) {
    let format = if !SUPPORTED_FORMATS.contains(&prefer_format.subtype()) {
        "text/html".parse().unwrap()
    } else {
        prefer_format.clone()
    };
    let content = match format.subtype().as_ref() {
        "plain" => status_error_plain(err.code, &err.name, err.summary.as_deref(), err.detail.as_deref()),
        "json" => status_error_json(err.code, &err.name, err.summary.as_deref(), err.detail.as_deref()),
        "xml" => status_error_xml(err.code, &err.name, err.summary.as_deref(), err.detail.as_deref()),
        _ => status_error_html(
            err.code,
            &err.name,
            err.summary.as_deref(),
            err.detail.as_deref(),
            footer,
        ),
    };
    (format, content.as_bytes().to_owned())
}

/// Default implementation of [`Catcher`].
///
/// If http status is error, and user is not set custom catcher to catch them,
/// `write_error_default` will used to catch them.
///
/// `Catcher` supports sending error pages in `XML`, `JSON`, `HTML`, `Text` formats.
pub struct Catcher {
    hoops: Vec<Arc<dyn Handler>>,
    handler: Arc<dyn Handler>,
}
impl Default for Catcher {
    fn default() -> Self {
        Catcher {
            handler: Arc::new(DefaultHandler::new()),
            hoops: vec![],
        }
    }
}
impl Catcher {
    /// Create new `Catcher`.
    pub fn new<H: Into<Arc<dyn Handler>>>(handler: H) -> Self {
        Catcher {
            handler: handler.into(),
            hoops: vec![],
        }
    }

    /// Get current catcher's middlewares reference.
    #[inline]
    pub fn hoops(&self) -> &Vec<Arc<dyn Handler>> {
        &self.hoops
    }
    /// Get current catcher's middlewares mutable reference.
    #[inline]
    pub fn hoops_mut(&mut self) -> &mut Vec<Arc<dyn Handler>> {
        &mut self.hoops
    }

    /// Add a handler as middleware, it will run the handler in current router or it's descendants
    /// handle the request.
    #[inline]
    pub fn hoop<H: Handler>(mut self, handler: H) -> Self {
        self.hoops.push(Arc::new(handler));
        self
    }

    /// Add a handler as middleware, it will run the handler in current router or it's descendants
    /// handle the request. This middleware only effective when the filter return true.
    #[inline]
    pub fn hoop_when<H, F>(mut self, handler: H, filter: F) -> Self
    where
        H: Handler,
        F: Fn(&Request, &Depot) -> bool + Send + Sync + 'static,
    {
        self.hoops.push(Arc::new(WhenHoop { inner: handler, filter }));
        self
    }

    /// Catch error and send error page.
    pub async fn catch(&self, req: &mut Request, depot: &mut Depot, res: &mut Response) {
        let mut ctrl = FlowCtrl::new(self.hoops.iter().chain([&self.handler]).cloned().collect());
        ctrl.call_next(req, depot, res).await;
    }
}

impl<H> From<H> for Catcher
where
    H: Into<Arc<dyn Handler>>,
{
    fn from(handler: H) -> Self {
        Catcher::new(handler)
    }
}

/// Default [`Handler`] for [`Catcher`].
///
/// If http status is error, and user is not set custom catcher to catch them,
/// `write_error_default` will used to catch them.
///
/// `Catcher` supports sending error pages in `XML`, `JSON`, `HTML`, `Text` formats.
#[derive(Default)]
pub struct DefaultHandler {
    footer: Option<Cow<'static, str>>,
}
impl DefaultHandler {
    /// Create new `Catcher`.
    pub fn new() -> Self {
        DefaultHandler { footer: None }
    }
    /// Create with footer.
    #[inline]
    pub fn with_footer(footer: impl Into<Cow<'static, str>>) -> Self {
        Self::new().footer(footer)
    }

    /// Set footer.
    pub fn footer(mut self, footer: impl Into<Cow<'static, str>>) -> Self {
        self.footer = Some(footer.into());
        self
    }
}
#[async_trait]
impl Handler for DefaultHandler {
    async fn handle(&self, req: &mut Request, _depot: &mut Depot, res: &mut Response, _ctrl: &mut FlowCtrl) {
        let status = res.status_code.unwrap_or(StatusCode::NOT_FOUND);
        if (status.is_server_error() || status.is_client_error()) && res.body.is_none() {
            write_error_default(req, res, self.footer.as_deref());
        }
    }
}

#[doc(hidden)]
pub fn write_error_default(req: &Request, res: &mut Response, footer: Option<&str>) {
    let format = guess_accept_mime(req, None);
    let (format, data) = if let ResBody::Error(body) = &res.body {
        status_error_bytes(body, &format, footer)
    } else {
        let status = res.status_code.unwrap_or(StatusCode::NOT_FOUND);
        status_error_bytes(&StatusError::from_code(status).unwrap(), &format, footer)
    };
    res.headers_mut()
        .insert(header::CONTENT_TYPE, format.to_string().parse().unwrap());
    res.write_body(data).ok();
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::test::{ResponseExt, TestClient};

    use super::*;

    struct CustomError;
    #[async_trait]
    impl Writer for CustomError {
        async fn write(mut self, _req: &mut Request, _depot: &mut Depot, res: &mut Response) {
            res.status_code = Some(StatusCode::INTERNAL_SERVER_ERROR);
            res.render("custom error");
        }
    }

    #[handler]
    async fn handle404(&self, _req: &Request, _depot: &Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
        if let Some(StatusCode::NOT_FOUND) = res.status_code {
            res.render("Custom 404 Error Page");
            ctrl.skip_rest();
        }
    }

    #[tokio::test]
    async fn test_handle_error() {
        #[handler]
        async fn handle_custom() -> Result<(), CustomError> {
            Err(CustomError)
        }
        let router = Router::new().push(Router::with_path("custom").get(handle_custom));
        let service = Service::new(router);

        async fn access(service: &Service, name: &str) -> String {
            TestClient::get(format!("http://127.0.0.1:5800/{}", name))
                .send(service)
                .await
                .take_string()
                .await
                .unwrap()
        }

        assert_eq!(access(&service, "custom").await, "custom error");
    }

    #[tokio::test]
    async fn test_custom_catcher() {
        #[handler]
        async fn hello() -> &'static str {
            "Hello World"
        }
        let router = Router::new().get(hello);
        let service = Service::new(router).catcher(Catcher::default().hoop(handle404));

        async fn access(service: &Service, name: &str) -> String {
            TestClient::get(format!("http://127.0.0.1:5800/{}", name))
                .send(service)
                .await
                .take_string()
                .await
                .unwrap()
        }

        assert_eq!(access(&service, "notfound").await, "Custom 404 Error Page");
    }
}
