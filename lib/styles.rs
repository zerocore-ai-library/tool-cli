//! CLI styles for clap.

use clap::builder::styling::{AnsiColor, Color, Style, Styles};

//--------------------------------------------------------------------------------------------------
// Macros
//--------------------------------------------------------------------------------------------------

/// Helper macro to generate styled example blocks for after_help.
/// Colors: yellow bold for headers, cyan for commands, dim italic for comments.
/// Usage: `examples!["cmd1" # "comment1", "cmd2" # "comment2"]`
#[macro_export]
macro_rules! examples {
    ($($cmd:literal $(# $comment:literal)?),* $(,)?) => {
        concat!(
            "\x1b[1;33mExamples:\x1b[0m",
            $("\n  \x1b[36m", $cmd, "\x1b[0m", $(" \x1b[2;3m# ", $comment, "\x1b[0m",)?)*
        )
    };
}

/// Helper macro for example blocks with a custom header (e.g., "Getting started:").
#[macro_export]
macro_rules! examples_section {
    ($header:literal; $($cmd:literal $(# $comment:literal)?),* $(,)?) => {
        concat!(
            "\x1b[1;33m", $header, "\x1b[0m",
            $("\n  \x1b[36m", $cmd, "\x1b[0m", $(" \x1b[2;3m# ", $comment, "\x1b[0m",)?)*
        )
    };
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

pub fn styles() -> Styles {
    Styles::styled()
        .header(
            Style::new()
                .bold()
                .fg_color(Some(Color::Ansi(AnsiColor::Yellow))),
        )
        .usage(
            Style::new()
                .bold()
                .fg_color(Some(Color::Ansi(AnsiColor::Green))),
        )
        .literal(Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))))
        .placeholder(Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))))
        .error(
            Style::new()
                .bold()
                .fg_color(Some(Color::Ansi(AnsiColor::Red))),
        )
        .invalid(
            Style::new()
                .bold()
                .fg_color(Some(Color::Ansi(AnsiColor::Red))),
        )
        .valid(
            Style::new()
                .bold()
                .fg_color(Some(Color::Ansi(AnsiColor::Green))),
        )
}
