use warp::Filter;

#[macro_use]
extern crate serde;

mod webhook;

use fern::colors::{Color, ColoredLevelConfig};
use webhook::Webhook;

fn setup_logger() {
    let colours_line = ColoredLevelConfig::new()
        .error(Color::Red)
        .warn(Color::Yellow)
        .info(Color::Green)
        .debug(Color::Blue)
        .trace(Color::BrightBlack);

    fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{colours_line}[{date}][{target}][{level}]\x1B[0m {message}",
                colours_line = format_args!(
                    "\x1B[{}m",
                    colours_line.get_color(&record.level()).to_fg_str()
                ),
                date = chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                target = record.target(),
                level = record.level(),
                message = message,
            ));
        })
        .level(log::LevelFilter::Warn)
        .level_for("fisherman", log::LevelFilter::Trace)
        .chain(std::io::stdout())
        .apply()
        .expect("Failed to initialise the logger");
}

#[tokio::main]
async fn main() {
    setup_logger();

    let webhook = warp::post().and(warp::body::json()).map(|body: Webhook| {
        log::debug!("Webhook body: {:?}", &body);

        if body.is_master_push() {
            log::info!("Commits were pushed to `master` in this event");
        }

        "Thanks for the update"
    });

    warp::serve(webhook).run(([127, 0, 0, 1], 5000)).await;
}
