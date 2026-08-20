use std::{sync::Arc, thread, time::Duration};

use crate::config::AtomicConfig;

pub struct Looper;

impl Looper {
    pub fn new() -> Self {
        Self
    }

    pub fn enter_loop(&mut self, config_manager: &Arc<AtomicConfig>) {
        loop {
            let _config = config_manager.get();
            thread::sleep(Duration::from_secs(1));
        }
    }
}
