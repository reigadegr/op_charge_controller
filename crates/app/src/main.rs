#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::{fmt, io::IsTerminal};

use chrono::Local;
use scheduler::Scheduler;
use tracing_subscriber::{
    EnvFilter,
    fmt::{format::Writer, time::FormatTime},
};

struct LoggerFormatter;

impl FormatTime for LoggerFormatter {
    fn format_time(&self, w: &mut Writer<'_>) -> fmt::Result {
        write!(w, "{}", Local::now().format("%Y-%m-%d %H:%M:%S"))
    }
}

fn main() -> anyhow::Result<()> {
    let env_filter = match EnvFilter::try_from_default_env() {
        Ok(env_filter) => env_filter,
        Err(_) => EnvFilter::new("info"),
    };

    let is_terminal = std::io::stdout().is_terminal();

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_timer(LoggerFormatter)
        .with_ansi(is_terminal)
        .init();

    Scheduler::new()?.start_run()
}
