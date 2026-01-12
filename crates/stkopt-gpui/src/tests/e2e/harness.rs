use gpui::*;
use gpui_ui_kit::harness::TestApp;
use std::sync::Arc;

use crate::app::StkoptApp;

pub struct StkoptTestApp {
    inner: TestApp,
}

impl StkoptTestApp {
    pub fn new() -> Self {
        Self {
            inner: TestApp::new(),
        }
    }

    pub async fn start(&mut self) -> Arc<View<StkoptApp>> {
        let (view, _cx) = self.inner.start_async(|cx| {
            // No special init needed for now, handled in StkoptApp::new
             StkoptApp::new(cx)
        }).await;
        view
    }

    pub fn inner(&mut self) -> &mut TestApp {
        &mut self.inner
    }
}
