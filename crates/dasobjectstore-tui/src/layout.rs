/// Terminal layout class used by the future renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalLayout {
    Compact,
    Standard,
}

/// Classifies a terminal into the supported Milestone 18 layout bands.
pub fn classify_terminal_layout(columns: u16, rows: u16) -> TerminalLayout {
    if columns < 100 || rows < 30 {
        TerminalLayout::Compact
    } else {
        TerminalLayout::Standard
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_terminal_layout, TerminalLayout};

    #[test]
    fn classifies_compact_layout_for_narrow_or_short_terminals() {
        assert_eq!(classify_terminal_layout(99, 40), TerminalLayout::Compact);
        assert_eq!(classify_terminal_layout(120, 29), TerminalLayout::Compact);
    }

    #[test]
    fn classifies_standard_layout_at_minimum_supported_size() {
        assert_eq!(classify_terminal_layout(100, 30), TerminalLayout::Standard);
    }
}
