//! Help Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

pub fn draw(f: &mut Frame, area: Rect, _app: &App) {
    let help_text = r#"
Vauchi TUI Help
================

Navigation
----------
  j/↓     Move down
  k/↑     Move up
  h/←     Move left (in dialogs)
  l/→     Move right (in dialogs)
  Enter   Select/confirm
  Esc     Go back/cancel
  Tab     Next field (in forms)

Home Screen
-----------
  e       Open exchange (QR code)
  c       View contacts
  s       Open settings
  a       Add new field
  d       Delete selected field

Contacts Screen
---------------
  Enter   View contact details

General
-------
  ?       Show this help
  q       Quit

Press Esc or q to close this help screen.
"#;

    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::White))
        .block(Block::default().title("Help").borders(Borders::ALL));

    f.render_widget(help, area);
}
