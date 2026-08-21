#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use scheduler::Scheduler;

fn main() -> anyhow::Result<()> {
    Scheduler::new()?.start_run()
}
