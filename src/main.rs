mod config;
mod scheduler;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use crate::scheduler::Scheduler;

fn main() -> anyhow::Result<()> {
    Scheduler::new()?.start_run();
    Ok(())
}
