//! Shared CLI styling (console / cliclack). `console` auto-disables ANSI when
//! the stream isn't a terminal or `NO_COLOR` is set, so these degrade cleanly
//! when output is piped or redirected to a daemon log.

use console::style;

/// Styled error line to stderr (used by the top-level handler).
pub fn fail(msg: &str) {
    eprintln!("{} {msg}", style("✗ error:").red().bold());
}

/// A bold banner for the start of a run mode / setup screen.
pub fn banner(title: &str, detail: &str) {
    println!(
        "\n{} {}",
        style(format!("▌ {title}")).cyan().bold(),
        style(detail).dim()
    );
}

/// A dim hint line.
pub fn hint(msg: &str) {
    println!("{}", style(msg).dim());
}
