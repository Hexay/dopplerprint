pub use dp_serve::ServeArgs as Args;

pub fn run(a: Args) -> anyhow::Result<()> {
    // TODO(merge): swap FakeBackend for the dp_core::app adapter (LocateRequest → app::locate → LocateOutcome).
    dp_serve::run(a, Box::new(dp_serve::FakeBackend::default()))
}
