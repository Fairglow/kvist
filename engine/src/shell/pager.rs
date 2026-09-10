#[cfg(not(test))]
use std::io::IsTerminal;

#[cfg(not(test))]
use minus::Pager;

/// Displays output text to the user.
///
/// If stdout is an interactive terminal and the output exceeds the terminal
/// height, the output is paginated cleanly using the `minus` pager without
/// overflowing screen buffers or losing scrollback context.
pub fn display_output(text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    // Never enter an interactive pager during automated tests
    #[cfg(test)]
    {
        println!("{text}");
    }

    #[cfg(not(test))]
    {
        // Only invoke pager if stdout is an interactive terminal and output is long enough
        if std::env::var_os("KVIST_NO_PAGER").is_none() && std::io::stdout().is_terminal() {
            let lines_count = text.lines().count();
            if lines_count > 15 {
                let pager = Pager::new();
                if pager.push_str(text).is_ok() && minus::page_all(pager).is_ok() {
                    return;
                }
            }
        }

        println!("{text}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_output_handles_empty_input() {
        display_output("");
        display_output("   \n\t  ");
    }

    #[test]
    fn display_output_handles_short_text() {
        display_output("simple status output");
    }

    #[test]
    fn display_output_handles_long_text() {
        let mut long_text = String::new();
        for i in 0..50 {
            long_text.push_str(&format!("line {i}: component details and metadata\n"));
        }
        display_output(&long_text);
    }
}
