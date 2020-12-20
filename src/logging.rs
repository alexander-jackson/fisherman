use fern::colors::{Color, ColoredLevelConfig};

pub fn setup_logger() {
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