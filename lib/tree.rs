//! Tree view generator for CLI commands.
//!
//! Generates a colored tree view of all commands and options.

use clap::Command;
use colored::*;
use std::fmt::Write;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Builder for generating tree views of CLI commands.
pub struct TreeBuilder {
    output: String,
    indent_stack: Vec<bool>,
    max_item_width: usize,
    prefix_cache: Vec<String>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl Default for TreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeBuilder {
    /// Create a new tree builder.
    pub fn new() -> Self {
        Self {
            output: String::with_capacity(4096),
            indent_stack: Vec::with_capacity(8),
            max_item_width: 0,
            prefix_cache: Vec::with_capacity(8),
        }
    }

    /// Build a tree view of the command.
    pub fn build(self, cmd: &Command) -> String {
        self.build_with_root(cmd, None)
    }

    /// Build a tree view with a custom root name.
    pub fn build_with_root(mut self, cmd: &Command, root_name: Option<&str>) -> String {
        // First pass: calculate max width for alignment
        self.calculate_max_width(cmd, 0);
        self.max_item_width += 2;

        // Pre-compute prefix strings
        self.build_prefix_cache();

        // Second pass: build the tree
        let root = root_name.unwrap_or_else(|| cmd.get_name());
        writeln!(&mut self.output, "{}", root.bold().yellow()).unwrap();
        self.build_tree(cmd, false);
        self.output
    }

    fn build_prefix_cache(&mut self) {
        for i in 0..8 {
            let mut prefix = String::with_capacity(i * 4 + 4);
            for j in 0..i {
                if j < i - 1 {
                    prefix.push_str("│   ");
                } else {
                    prefix.push_str("    ");
                }
            }
            self.prefix_cache.push(prefix);
        }
    }

    fn calculate_max_width(&mut self, cmd: &Command, depth: usize) {
        let indent_width = depth * 4;

        for subcmd in cmd.get_subcommands() {
            let name_len = subcmd.get_name().len();
            let aliases_count = subcmd.get_all_aliases().count();

            let mut width = indent_width + 4 + name_len;
            if aliases_count > 0 {
                width += 12 + aliases_count * 5;
            }

            self.max_item_width = self.max_item_width.max(width);
            self.calculate_max_width(subcmd, depth + 1);
        }

        for arg in cmd.get_arguments() {
            let id = arg.get_id().as_str();
            if id == "help" {
                continue;
            }

            let width = indent_width + 4 + self.calculate_flag_width_fast(arg);
            self.max_item_width = self.max_item_width.max(width);
        }
    }

    fn calculate_flag_width_fast(&self, arg: &clap::Arg) -> usize {
        let id = arg.get_id().as_str();

        if id == "version" {
            return if arg.get_short().is_some() && arg.get_long().is_some() {
                13
            } else if arg.get_long().is_some() {
                11
            } else if arg.get_short().is_some() {
                2
            } else {
                0
            };
        }

        let mut width = 0;

        if arg.get_short().is_none() && arg.get_long().is_none() {
            let value_name = arg
                .get_value_names()
                .and_then(|names| names.first().map(|n| n.as_str()))
                .unwrap_or(id);
            return value_name.len() + 2;
        }

        if arg.get_short().is_some() {
            width += 2;
            if arg.get_long().is_some() {
                width += 2;
            }
        }

        if let Some(long) = arg.get_long() {
            width += 2 + long.len();
        }

        if arg.get_num_args().is_some() || arg.get_action().takes_values() {
            if let Some(value_names) = arg.get_value_names() {
                if let Some(name) = value_names.first() {
                    width += 3 + name.len();
                }
            } else {
                width += 8;
            }
        }

        let short_aliases: Vec<_> = arg.get_visible_short_aliases().unwrap_or_default();
        let long_aliases: Vec<_> = arg.get_visible_aliases().unwrap_or_default();
        if !short_aliases.is_empty() || !long_aliases.is_empty() {
            width += 12;
            if !short_aliases.is_empty() {
                width += short_aliases.len() * 2 + (short_aliases.len() - 1) * 2;
                if !long_aliases.is_empty() {
                    width += 2;
                }
            }
            if !long_aliases.is_empty() {
                width += long_aliases.iter().map(|a| a.len() + 2).sum::<usize>()
                    + (long_aliases.len() - 1) * 2;
            }
        }

        width
    }

    fn build_tree(&mut self, cmd: &Command, _is_last: bool) {
        let global_opts = self.collect_options(cmd);
        let subcommands: Vec<_> = cmd.get_subcommands().collect();

        let total_items = global_opts.len() + subcommands.len();
        let mut item_index = 0;

        for (opt, desc) in &global_opts {
            item_index += 1;
            let is_last_item = item_index == total_items;
            self.print_item(opt, desc.as_deref(), is_last_item);
        }

        for subcmd in subcommands {
            item_index += 1;
            let is_last_item = item_index == total_items;
            self.print_subcommand(subcmd, is_last_item);
        }
    }

    fn print_item(&mut self, item: &str, description: Option<&str>, is_last: bool) {
        let prefix = self.build_prefix_fast(is_last);
        let colored_item = self.colorize_flag_or_arg(item);

        let current_indent = self.indent_stack.len() * 4;
        let prefix_width = 4;
        let item_width = self.calculate_visible_width(item);
        let total_width = current_indent + prefix_width + item_width;
        let padding_needed = self.max_item_width.saturating_sub(total_width);

        if let Some(desc) = description {
            write!(
                &mut self.output,
                "{}{}{:width$}{}",
                prefix.dimmed(),
                colored_item,
                "",
                desc.white(),
                width = padding_needed
            )
            .unwrap();
        } else {
            write!(&mut self.output, "{}{}", prefix.dimmed(), colored_item).unwrap();
        }
        writeln!(&mut self.output).unwrap();
    }

    fn calculate_visible_width(&self, text: &str) -> usize {
        text.len()
    }

    fn colorize_flag_or_arg(&self, text: &str) -> String {
        if text.starts_with('-') {
            if let Some((flag_part, alias_part)) = text.split_once(" (aliases: ") {
                let flag_colored = if let Some((flag, value)) = flag_part.split_once(' ') {
                    format!("{} {}", flag.bright_black(), value.bright_black())
                } else {
                    flag_part.bright_black().to_string()
                };
                format!(
                    "{} {}",
                    flag_colored,
                    format!("(aliases: {}", alias_part).dimmed()
                )
            } else if let Some((flag, value)) = text.split_once(' ') {
                format!("{} {}", flag.bright_black(), value.bright_black())
            } else {
                text.bright_black().to_string()
            }
        } else if (text.starts_with('<') && text.ends_with('>'))
            || (text.starts_with('[') && text.ends_with(']'))
        {
            text.bright_black().to_string()
        } else {
            text.to_string()
        }
    }

    fn print_subcommand(&mut self, cmd: &Command, is_last: bool) {
        let prefix = self.build_prefix_fast(is_last);
        let name = cmd.get_name();

        let aliases: Vec<_> = cmd.get_all_aliases().collect();
        let has_aliases = !aliases.is_empty();

        let current_indent = self.indent_stack.len() * 4;
        let prefix_width = 4;
        let visible_width = if has_aliases {
            name.len()
                + 12
                + aliases.iter().map(|a| a.len()).sum::<usize>()
                + (aliases.len() - 1) * 2
        } else {
            name.len()
        };
        let total_width = current_indent + prefix_width + visible_width;
        let padding_needed = self.max_item_width.saturating_sub(total_width);

        let colored_name = match self.indent_stack.len() {
            0 => name.magenta().bold(),
            1 => name.blue().bold(),
            2 => name.green().bold(),
            3 => name.cyan().bold(),
            _ => name.bright_green().bold(),
        };

        write!(&mut self.output, "{}{}", prefix.dimmed(), colored_name).unwrap();

        if has_aliases {
            write!(
                &mut self.output,
                " {}",
                format!("(aliases: {})", aliases.join(", ")).dimmed()
            )
            .unwrap();
        }

        if let Some(about) = cmd.get_about() {
            let about_str = about.to_string();
            if let Some(first_line) = about_str.lines().next() {
                if !first_line.is_empty() {
                    write!(
                        &mut self.output,
                        "{:width$}{}",
                        "",
                        first_line.white(),
                        width = padding_needed
                    )
                    .unwrap();
                }
            }
        }

        writeln!(&mut self.output).unwrap();

        if cmd.has_subcommands() || self.has_visible_args(cmd) {
            self.indent_stack.push(!is_last);
            self.build_tree(cmd, is_last);
            self.indent_stack.pop();
        }
    }

    fn build_prefix_fast(&self, is_last: bool) -> String {
        let depth = self.indent_stack.len();
        let mut prefix = String::with_capacity(depth * 4 + 4);

        for &continues in &self.indent_stack {
            if continues {
                prefix.push_str("│   ");
            } else {
                prefix.push_str("    ");
            }
        }

        if is_last {
            prefix.push_str("└── ");
        } else {
            prefix.push_str("├── ");
        }

        prefix
    }

    fn collect_options(&self, cmd: &Command) -> Vec<(String, Option<String>)> {
        let mut options = Vec::with_capacity(cmd.get_arguments().size_hint().0);

        for arg in cmd.get_arguments() {
            let id = arg.get_id().as_str();

            if id == "help" {
                continue;
            }

            let mut opt_str = String::with_capacity(32);
            let mut desc_str = None;

            if id == "version" {
                if arg.get_short().is_some() || arg.get_long().is_some() {
                    if let Some(short) = arg.get_short() {
                        write!(&mut opt_str, "-{}", short).unwrap();
                        if arg.get_long().is_some() {
                            opt_str.push_str(", ");
                        }
                    }
                    if let Some(long) = arg.get_long() {
                        write!(&mut opt_str, "--{}", long).unwrap();
                    }

                    if let Some(help) = arg.get_help() {
                        let help_str = help.to_string();
                        if let Some(first_line) = help_str.lines().next() {
                            if !first_line.is_empty() {
                                desc_str = Some(first_line.to_string());
                            }
                        }
                    }
                    options.push((opt_str, desc_str));
                }
                continue;
            }

            if arg.get_short().is_none() && arg.get_long().is_none() {
                let value_name = arg
                    .get_value_names()
                    .and_then(|names| names.first().map(|n| n.as_str()))
                    .unwrap_or(id);

                if arg.is_required_set() {
                    write!(&mut opt_str, "<{}>", value_name.to_uppercase()).unwrap();
                } else {
                    write!(&mut opt_str, "[{}]", value_name.to_uppercase()).unwrap();
                }

                if let Some(help) = arg.get_help() {
                    let help_str = help.to_string();
                    if let Some(first_line) = help_str.lines().next() {
                        if !first_line.is_empty() {
                            desc_str = Some(first_line.to_string());
                        }
                    }
                }
                options.push((opt_str, desc_str));
                continue;
            }

            if let Some(short) = arg.get_short() {
                write!(&mut opt_str, "-{}", short).unwrap();
                if arg.get_long().is_some() {
                    opt_str.push_str(", ");
                }
            }

            if let Some(long) = arg.get_long() {
                write!(&mut opt_str, "--{}", long).unwrap();
            }

            if arg.get_num_args().is_some() || arg.get_action().takes_values() {
                if let Some(value_names) = arg.get_value_names() {
                    if let Some(name) = value_names.first() {
                        write!(&mut opt_str, " <{}>", name.to_uppercase()).unwrap();
                    }
                } else {
                    opt_str.push_str(" <VALUE>");
                }
            }

            let short_aliases: Vec<_> = arg.get_visible_short_aliases().unwrap_or_default();
            let long_aliases: Vec<_> = arg.get_visible_aliases().unwrap_or_default();
            if !short_aliases.is_empty() || !long_aliases.is_empty() {
                opt_str.push_str(" (aliases: ");
                let mut first = true;
                for alias in &short_aliases {
                    if !first {
                        opt_str.push_str(", ");
                    }
                    write!(&mut opt_str, "-{}", alias).unwrap();
                    first = false;
                }
                for alias in &long_aliases {
                    if !first {
                        opt_str.push_str(", ");
                    }
                    write!(&mut opt_str, "--{}", alias).unwrap();
                    first = false;
                }
                opt_str.push(')');
            }

            if let Some(help) = arg.get_help() {
                let help_str = help.to_string();
                if let Some(first_line) = help_str.lines().next() {
                    if !first_line.is_empty() {
                        desc_str = Some(first_line.to_string());
                    }
                }
            }

            options.push((opt_str, desc_str));
        }

        options
    }

    fn has_visible_args(&self, cmd: &Command) -> bool {
        cmd.get_arguments().any(|arg| {
            let id = arg.get_id().as_str();
            if id == "help" || id == "version" {
                arg.is_global_set()
            } else {
                arg.get_short().is_some()
                    || arg.get_long().is_some()
                    || (arg.get_short().is_none() && arg.get_long().is_none())
            }
        })
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Generate a tree view of all commands and options.
pub fn generate_tree(cmd: &Command) -> String {
    TreeBuilder::new().build(cmd)
}

/// Generate a tree view with a custom root name.
pub fn generate_tree_with_root(cmd: &Command, root_name: &str) -> String {
    TreeBuilder::new().build_with_root(cmd, Some(root_name))
}

/// Generate a tree view if the --tree flag is present in command-line arguments.
///
/// This function checks for the --tree flag, finds the appropriate command level,
/// and generates the tree. It must be called before clap's `parse()` to avoid
/// errors with required arguments.
///
/// Returns `Some(tree_string)` if --tree flag was found, `None` otherwise.
pub fn try_show_tree(cmd: &Command) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    try_show_tree_from_args(cmd, &args)
}

/// Same as `try_show_tree` but accepts args directly instead of reading from env.
pub fn try_show_tree_from_args(cmd: &Command, args: &[String]) -> Option<String> {
    if !args.iter().any(|arg| arg == "--tree") {
        return None;
    }

    let (path, deepest_cmd) = find_deepest_command_from_args(cmd, args);

    let tree = if path.len() > 1 {
        generate_tree_with_root(&deepest_cmd, &path.join(" "))
    } else {
        generate_tree(&deepest_cmd)
    };

    Some(tree)
}

/// Find the deepest command in the hierarchy based on the command-line arguments.
fn find_deepest_command_from_args(cmd: &Command, args: &[String]) -> (Vec<String>, Command) {
    let mut path = vec![cmd.get_name().to_string()];
    let mut current_cmd = cmd.clone();

    let has_tree_flag = args.iter().any(|arg| arg == "--tree");

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if arg.starts_with('-') {
            break;
        }

        if let Some(subcmd) = current_cmd.find_subcommand(arg) {
            path.push(arg.to_string());
            current_cmd = subcmd.clone();
            i += 1;
        } else if has_tree_flag {
            i += 1;
            continue;
        } else {
            break;
        }
    }

    (path, current_cmd)
}
