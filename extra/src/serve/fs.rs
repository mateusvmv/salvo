use async_trait::async_trait;

use salvo_core::{Request, Response, Depot, Writer};
use salvo_core::fs::NamedFile;
use salvo_core::Handler;

#[derive(Clone)]
pub struct StaticFile(String);
impl StaticFile {
    pub fn new(path: impl Into<String>) -> Self {
        StaticFile(path.into())
    }
}

#[async_trait]
impl Handler for StaticFile {
    async fn handle(&self, req: &mut Request, depot: &mut Depot, res: &mut Response) {
        let named_file = NamedFile::open(self.0.clone().into());
        if named_file.is_err() {
            res.not_found();
            return;
        }
        let named_file = named_file.unwrap();
        named_file.write(req, depot, res).await;
    }
}