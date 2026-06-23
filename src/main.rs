mod application;
mod capture;
mod portal;
mod settings;
mod window;

#[allow(dead_code)]
mod config {
    include!(concat!(env!("OUT_DIR"), "/config.rs"));
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"))
        )
        .init();

    gst::init().expect("Failed to initialise GStreamer");
    relm4::adw::init().expect("Failed to initialise libadwaita");

    application::run();
}
