//! CLI styles for clap.

use clap::builder::styling::{AnsiColor, Color, Style, Styles};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Spinner tick characters (dots3 style).
const SPINNER_TICKS: &[&str] = &["⠄", "⠆", "⠇", "⠋", "⠙", "⠸", "⠰", "⠠"];

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// A reusable CLI spinner for async operations.
pub struct Spinner {
    pb: ProgressBar,
    action: String,
    indent: usize,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl Spinner {
    /// Create and start a new spinner with the given message.
    ///
    /// The spinner is indented with 2 spaces to align with standard CLI output.
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_indent(message, 2)
    }

    /// Create and start a new spinner with custom indentation.
    ///
    /// Use `indent=2` for standard operations (default).
    pub fn with_indent(message: impl Into<String>, indent: usize) -> Self {
        let message = message.into();
        let pb = ProgressBar::new_spinner();
        let template = format!("{:indent$}{{spinner:.cyan}} {{msg}}", "", indent = indent);
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(SPINNER_TICKS)
                .template(&template)
                .unwrap(),
        );
        pb.set_message(message.clone());
        pb.enable_steady_tick(Duration::from_millis(80));

        Self {
            pb,
            action: message,
            indent,
        }
    }

    /// Finish the spinner with a success message.
    ///
    /// Displays: `✓ {message}` or `✓ {action}` if no message provided.
    /// The message is indented to match the spinner's original position.
    pub fn succeed(self, message: Option<&str>) {
        self.pb.finish_and_clear();
        let msg = message
            .map(|m| m.to_string())
            .unwrap_or_else(|| self.action.clone());
        println!(
            "{:indent$}{} {}",
            "",
            "✓".bright_green(),
            msg,
            indent = self.indent
        );
    }

    /// Finish the spinner successfully (clears the line).
    ///
    /// Use this ONLY when a separate success message will be printed immediately after.
    /// For most cases, prefer `succeed()` which leaves a visible success indicator.
    pub fn done(self) {
        self.pb.finish_and_clear();
    }

    /// Finish the spinner with a failure message.
    ///
    /// Displays: `✗ {message}` or `✗ {action} failed` if no message provided.
    /// The message is indented to match the spinner's original position.
    pub fn fail(self, message: Option<&str>) {
        self.pb.finish_and_clear();
        let msg = message
            .map(|m| m.to_string())
            .unwrap_or_else(|| format!("{} failed", self.action));
        println!(
            "{:indent$}{} {}",
            "",
            "✗".bright_red(),
            msg,
            indent = self.indent
        );
    }
}

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
