//! Shared CLI styling (console / cliclack). `console` auto-disables ANSI when
//! the stream isn't a terminal or `NO_COLOR` is set, so these degrade cleanly
//! when output is piped or redirected to a daemon log.

use console::{measure_text_width, style};

/// Styled error line to stderr (used by the top-level handler).
pub fn fail(msg: &str) {
    eprintln!("{} {msg}", style("✗ error:").red().bold());
}

/// A boxen-style rounded box around `lines`, with `title` set into the top
/// border. Widths are measured with the ANSI escapes stripped, so coloured
/// content stays aligned. Falls back to plain lines when output isn't a
/// terminal (piped / redirected to a log), keeping it grep-friendly.
pub fn boxed(title: &str, lines: &[String]) {
    if !console::user_attended() {
        for l in lines {
            println!("{l}");
        }
        return;
    }
    const PAD: usize = 1;
    let content_w = lines
        .iter()
        .map(|l| measure_text_width(l))
        .max()
        .unwrap_or(0);
    let title_seg = if title.is_empty() {
        String::new()
    } else {
        format!(" {} ", style(title).bold())
    };
    let title_w = measure_text_width(&title_seg);
    // Interior = chars between the corner glyphs; fit both content and title.
    let interior = (content_w + PAD * 2).max(title_w + 1);
    let edge = |s: String| style(s).cyan().to_string();

    // Top: ╭─<title>──…──╮
    let rest = interior - 1 - title_w;
    println!(
        "{}{}{}",
        edge("╭─".into()),
        title_seg,
        edge(format!("{}╮", "─".repeat(rest)))
    );
    // Content rows, right-padded to the interior width (minus padding).
    let bar = edge("│".into());
    let pad = " ".repeat(PAD);
    for l in lines {
        let fill = " ".repeat(interior - PAD * 2 - measure_text_width(l));
        println!("{bar}{pad}{l}{fill}{pad}{bar}");
    }
    println!("{}", edge(format!("╰{}╯", "─".repeat(interior))));
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
