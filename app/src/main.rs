#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use scheduler::Scheduler;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(|buffer, record| {
            use std::io::Write;

            writeln!(
                buffer,
                "[{}] {}: {}",
                buffer.timestamp_seconds(),
                record.level(),
                record.args()
            )
        })
        .init();

    Scheduler::new()?.start_run()
}
