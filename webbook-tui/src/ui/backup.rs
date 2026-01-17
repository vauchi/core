//! Backup Screen UI

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, BackupFocus, BackupMode};

/// Draw the backup/restore screen.
pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    match app.backup_state.mode {
        BackupMode::Menu => draw_menu(f, area),
        BackupMode::Export => draw_export(f, area, app),
        BackupMode::Import => draw_import(f, area, app),
    }
}

fn draw_menu(f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Info
            Constraint::Length(3),  // Export option
            Constraint::Length(3),  // Import option
            Constraint::Min(0),     // Spacer
        ])
        .split(area);

    let info = Paragraph::new(
        "Back up your identity to transfer to another device or restore after reinstalling.\n\
         Your backup is encrypted with a password you choose."
    )
    .style(Style::default().fg(Color::Cyan))
    .block(Block::default().borders(Borders::ALL).title("Backup & Restore"));
    f.render_widget(info, chunks[0]);

    let export = Paragraph::new("[e] Export Backup - Create an encrypted backup")
        .style(Style::default().fg(Color::Green))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(export, chunks[1]);

    let import = Paragraph::new("[i] Import Backup - Restore from a backup")
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(import, chunks[2]);
}

fn draw_export(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Info
            Constraint::Length(3),  // Password
            Constraint::Length(3),  // Confirm
            Constraint::Length(3),  // Instructions
            Constraint::Min(0),     // Spacer
        ])
        .split(area);

    let info = Paragraph::new("Enter a password to encrypt your backup (min 8 characters):")
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(info, chunks[0]);

    let pw_style = if app.backup_state.focus == BackupFocus::Password {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let password = Paragraph::new(format!(
        "Password: {}",
        "*".repeat(app.backup_state.password.len())
    ))
    .style(pw_style)
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(password, chunks[1]);

    let confirm_style = if app.backup_state.focus == BackupFocus::Confirm {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let confirm = Paragraph::new(format!(
        "Confirm:  {}",
        "*".repeat(app.backup_state.confirm_password.len())
    ))
    .style(confirm_style)
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(confirm, chunks[2]);

    let instructions = Paragraph::new("[Tab] switch fields  [Enter] export  [Esc] cancel")
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(instructions, chunks[3]);
}

fn draw_import(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Info
            Constraint::Length(5),  // Data
            Constraint::Length(3),  // Password
            Constraint::Length(3),  // Instructions
            Constraint::Min(0),     // Spacer
        ])
        .split(area);

    let info = Paragraph::new("Paste your backup data and enter the password used to create it:")
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(info, chunks[0]);

    let data_style = if app.backup_state.focus == BackupFocus::Data {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let data_display = if app.backup_state.backup_data.len() > 40 {
        format!("{}...", &app.backup_state.backup_data[..40])
    } else {
        app.backup_state.backup_data.clone()
    };
    let data = Paragraph::new(format!("Backup data: {}", data_display))
        .style(data_style)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(data, chunks[1]);

    let pw_style = if app.backup_state.focus == BackupFocus::Password {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let password = Paragraph::new(format!(
        "Password: {}",
        "*".repeat(app.backup_state.password.len())
    ))
    .style(pw_style)
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(password, chunks[2]);

    let instructions = Paragraph::new("[Tab] switch fields  [Enter] import  [Esc] cancel")
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(instructions, chunks[3]);
}
