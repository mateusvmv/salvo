//! serve static dir and file middleware

pub mod dir;
mod embed;
mod file;

use percent_encoding::{utf8_percent_encode, CONTROLS};

pub use dir::{StaticDir, StaticDirOptions};
pub use embed::{static_embed, StaticEmbed};
pub use file::StaticFile;

#[inline]
pub(crate) fn encode_url_path(path: &str) -> String {
    path.split('/')
        .map(|s| utf8_percent_encode(s, CONTROLS).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

#[inline]
pub(crate) fn decode_url_path_safely(path: &str) -> String {
    percent_encoding::percent_decode_str(path)
        .decode_utf8_lossy()
        .to_string()
}

#[inline]
pub(crate) fn format_url_path_safely(path: &str) -> String {
    let mut used_parts = Vec::with_capacity(8);
    for part in path.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        } else if part == ".." {
            used_parts.pop();
        } else {
            used_parts.push(part);
        }
    }
    used_parts.join("/")
}

#[cfg(test)]
mod tests {
    use crate::serve_static::*;
    use salvo_core::prelude::*;
    use salvo_core::test::{ResponseExt, TestClient};

    #[tokio::test]
    async fn test_serve_static_files() {
        let router = Router::with_path("<**path>").get(StaticDir::width_options(
            vec!["../examples/static-dir-list/static/test"],
            StaticDirOptions {
                dot_files: false,
                listing: true,
                defaults: vec!["index.html".to_owned()],
            },
        ));
        let service = Service::new(router);

        async fn access(service: &Service, accept: &str, url: &str) -> String {
            TestClient::get(url)
                .add_header("accept", accept, true)
                .send(service)
                .await
                .take_string()
                .await
                .unwrap()
        }
        let content = access(&service, "text/plain", "http://127.0.0.1:7979/").await;
        assert!(content.contains("test1.txt") && content.contains("test2.txt"));

        let content = access(&service, "text/xml", "http://127.0.0.1:7979/").await;
        assert!(content.starts_with("<list>") && content.contains("test1.txt") && content.contains("test2.txt"));

        let content = access(&service, "text/html", "http://127.0.0.1:7979/").await;
        assert!(content.contains("<html>") && content.contains("test1.txt") && content.contains("test2.txt"));

        let content = access(&service, "application/json", "http://127.0.0.1:7979/").await;
        assert!(content.starts_with('{') && content.contains("test1.txt") && content.contains("test2.txt"));

        let content = access(&service, "text/plain", "http://127.0.0.1:7979/test1.txt").await;
        assert!(content.contains("copy1"));

        let content = access(&service, "text/plain", "http://127.0.0.1:7979/test3.txt").await;
        assert!(content.contains("Not Found"));

        let content = access(&service, "text/plain", "http://127.0.0.1:7979/../girl/love/eat.txt").await;
        assert!(content.contains("Not Found"));
        let content = access(&service, "text/plain", "http://127.0.0.1:7979/..\\girl\\love\\eat.txt").await;
        assert!(content.contains("Not Found"));

        let content = access(&service, "text/plain", "http://127.0.0.1:7979/dir1/test3.txt").await;
        assert!(content.contains("copy3"));
        let content = access(&service, "text/plain", "http://127.0.0.1:7979/dir1/dir2/test3.txt").await;
        assert!(content == "dir2 test3");
        let content = access(&service, "text/plain", "http://127.0.0.1:7979/dir1/../dir1/test3.txt").await;
        assert!(content == "copy3");
        let content = access(
            &service,
            "text/plain",
            "http://127.0.0.1:7979/dir1\\..\\dir1\\test3.txt",
        )
        .await;
        assert!(content == "copy3");
    }
}