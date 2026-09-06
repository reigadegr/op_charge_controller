pub mod looper;
mod ramp_up;
mod taper;

use std::{sync::Arc, thread, time::Duration};

use anyhow::Result;
use config::{AtomicConfig, profile_path};
use inotify::{Inotify, WatchMask};
use looper::Looper;
use tracing::{error, info};

pub struct Scheduler {
    atomic_config: Arc<AtomicConfig>,
}

impl Scheduler {
    pub fn new() -> Result<Self> {
        let atomic_config = Arc::new(AtomicConfig::init()?);

        Ok(Self { atomic_config })
    }

    fn start_config_watcher(&self) {
        let config = Arc::clone(&self.atomic_config);

        std::thread::spawn(move || {
            let config_path = profile_path();
            let mut inotify = match Inotify::init() {
                Ok(i) => i,
                Err(e) => {
                    error!("Failed to initialize inotify for config watcher: {e}");
                    return;
                }
            };

            if let Err(e) = inotify.watches().add(config_path, WatchMask::CLOSE_WRITE) {
                error!("Failed to add watch for config file: {e}");
                return;
            }

            info!("Config watcher started");

            loop {
                match inotify.read_events_blocking(&mut [0; 1024]) {
                    Ok(_) => {
                        thread::sleep(Duration::from_millis(50));
                        config.reload();
                    }
                    Err(e) => {
                        error!("Failed to read inotify events: {e}");
                        thread::sleep(Duration::from_secs(1));
                    }
                }
            }
        });
    }

    pub fn start_run(&self) -> Result<()> {
        self.start_config_watcher();
        Looper::new().enter_loop(&self.atomic_config)
    }
}
