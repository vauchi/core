//! Exchange Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Instructions
            Constraint::Min(0),    // QR code
        ])
        .split(area);

    let instructions = Paragraph::new("Share this QR code with others to exchange contact cards")
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(instructions, chunks[0]);

    // Generate and display QR code
    match app.backend.generate_exchange_qr() {
        Ok(data) => {
            let qr_display = render_qr_ascii(&data);
            let qr = Paragraph::new(qr_display)
                .style(Style::default().fg(Color::White))
                .block(Block::default().title("Your QR Code").borders(Borders::ALL));
            f.render_widget(qr, chunks[1]);
        }
        Err(e) => {
            let error = Paragraph::new(format!("Error: {}", e))
                .style(Style::default().fg(Color::Red))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(error, chunks[1]);
        }
    }
}

/// Render a QR code as ASCII art.
fn render_qr_ascii(data: &str) -> String {
    use qrcode::QrCode;

    match QrCode::new(data) {
        Ok(code) => code
            .render()
            .dark_color('█')
            .light_color(' ')
            .quiet_zone(true)
            .build(),
        Err(_) => "Failed to generate QR code".to_string(),
    }
}
