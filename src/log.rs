use tracing::{Level};
use tracing_subscriber::FmtSubscriber;

pub fn setup_trace_logging() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();
}