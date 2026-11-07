//! The bridge's log lines, on stdout as in the Go bridge. Message content
//! (text, emoji, file names, error texts that may contain them) is only
//! logged with `log_content`, and control characters are always removed, so
//! a message cannot inject terminal escape codes.
//!
//! [`clean`] removes them from values (also line breaks, so a value cannot
//! start a line that looks like the bridge's own). [`line`] removes them
//! again from the whole line, keeping only the line breaks the bridge's own
//! text has (the QR code): a safety net for error texts from the library,
//! the store or the network that are logged as they are.

/// Writes one log line.
#[allow(clippy::print_stdout)] // the bridge's log; stdout carries no protocol here
pub fn line(text: &str) {
    println!("{}", printable(text));
}

/// The text without control characters other than line breaks.
fn printable(text: &str) -> String {
    text.chars()
        .filter(|&c| c == '\n' || !c.is_control())
        .collect()
}

/// Removes control characters (Go's `unicode.IsControl`: C0, DEL and C1).
pub fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// The line for the outcome of a send: the status text only with content
/// logging, because it can contain content.
pub fn result_line(what: &str, success: bool, status: &str, with_content: bool) -> String {
    if with_content {
        format!("{what} {success} {}", clean(status))
    } else if success {
        format!("{what} true")
    } else {
        format!("{what} false (set WHATSAPP_LOG_CONTENT=1 to log the error)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ported from log_test.go.
    #[test]
    fn result_lines() {
        assert_eq!(
            result_line("Reaction sent", true, "Reaction 👍 sent", false),
            "Reaction sent true"
        );
        assert_eq!(
            result_line(
                "Message sent",
                false,
                "Error reading media file: open /home/me/outbox/salary.pdf",
                false
            ),
            "Message sent false (set WHATSAPP_LOG_CONTENT=1 to log the error)"
        );
        assert_eq!(
            result_line("Reaction sent", true, "Reaction 👍 sent", true),
            "Reaction sent true Reaction 👍 sent"
        );
    }

    #[test]
    fn control_characters_are_removed() {
        assert_eq!(clean("a\u{1b}[31mb\nc\u{7f}d\u{85}e"), "a[31mbcde");
        assert_eq!(clean("Noor 👍 café"), "Noor 👍 café");
    }

    #[test]
    fn whole_lines_keep_only_line_breaks() {
        assert_eq!(
            printable("\nScan this:\n▀▄\u{1b}]0;title\u{7}\r\n"),
            "\nScan this:\n▀▄]0;title\n"
        );
    }
}
