/// Sets up the logging for the application.
pub fn setup_logger() {
    if std::env::var("RUST_LOG").is_err() {
        // Set a reasonable default for logging in production
        std::env::set_var("RUST_LOG", "info,fisherman=debug");
    }

    tracing_subscriber::fmt::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
}
