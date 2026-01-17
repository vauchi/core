//! Settings Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

pub fn draw(f: &mut Frame, area: Rect, _app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Backup section
            Constraint::Length(5), // Import section
            Constraint::Min(0),    // Spacer
        ])
        .margin(1)
        .split(area);

    let backup_text = "Press 'b' to export a backup of your identity\n\
                       The backup will be saved to webbook-backup.json";
    let backup =
        Paragraph::new(backup_text).block(Block::default().title("Backup").borders(Borders::ALL));
    f.render_widget(backup, chunks[0]);

    let import_text = "Press 'i' to import a backup\n\
                       You will need the backup file and password";
    let import =
        Paragraph::new(import_text).block(Block::default().title("Import").borders(Borders::ALL));
    f.render_widget(import, chunks[1]);
}
