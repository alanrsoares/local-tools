//! Records every scaffold run as a one-line TSV entry so the user can see what
//! they generated and when, e.g.:
//!
//!    1735699200   ts   web   /Users/alana/dev/web
//!
//! The log lives under the tool's config dir (`~/.config/local-tools/scaffold`)
//! and is *best-effort*: a logging failure must never fail a scaffold.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::templates::Lang;

/// Build a one-line, tab-separated record.
///
/// Pure (takes its own timestamp) so it is trivially unit-testable.
pub fn record_line(secs: &str, lang: Lang, name: &str, target: &Path) -> String {
    format!("{secs}\t{}\t{}\t{}\n", lang, name, target.display())
}

/// Append `line` to `recent.log` under `dir`, creating the directory tree if
/// needed. Returns the underlying io result; callers ignore errors so a missing
/// or unreadable config dir cannot break a scaffold.
pub fn append(dir: &Path, line: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let log_path = dir.join("recent.log");
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    f.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_line_is_tab_separated_and_terminates_with_newline() {
        let target = Path::new("/tmp/proj");
        let line = record_line("123", Lang::Ts, "web", target);
        assert!(line.ends_with('\n'));
        assert_eq!(line, "123\tts\tweb\t/tmp/proj\n");
    }
}
