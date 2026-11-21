use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::io::IsTerminal;
use std::time::Duration;

pub fn make_bar(total: u64, enabled: bool, start: u64, message: impl Into<String>) -> ProgressBar {
    let bar = ProgressBar::new(total);
    if !enabled || !std::io::stderr().is_terminal() {
        bar.set_draw_target(ProgressDrawTarget::hidden());
    } else {
        let style = ProgressStyle::with_template(
            "{spinner} {msg} {bytes}/{total_bytes} ({percent}%) {bytes_per_sec} ETA {eta}",
        )
        .expect("plantilla de progreso inválida")
        .progress_chars("=> ");
        bar.set_style(style);
        bar.enable_steady_tick(Duration::from_millis(250));
    }
    bar.set_position(start);
    bar.set_message(message.into());
    bar
}
