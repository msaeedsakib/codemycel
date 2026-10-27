use tracing_subscriber::EnvFilter;

pub const LOG_ENV: &str = "CODEMYCEL_LOG";

/// Installs a stderr-only subscriber filtered by `CODEMYCEL_LOG` (default `warn`); stdout belongs to MCP and command output.
/// A second call keeps the first subscriber.
pub fn init_logging() {
    let filter = EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new("warn"));
    let _already_installed = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
