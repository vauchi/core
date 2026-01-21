//! Device Management Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::App;

/// State for device link display
#[derive(Default)]
#[allow(dead_code)]
pub struct DeviceLinkState {
    pub qr_data: Option<String>,
    pub show_qr: bool,
}

/// Draw the devices screen.
pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6), // Info + instructions
            Constraint::Min(0),    // Device list
            Constraint::Length(3), // Key hints
        ])
        .split(area);

    // Device info section
    draw_info_section(f, main_chunks[0], app);

    // Device list section
    draw_device_list(f, main_chunks[1], app);

    // Key hints
    draw_key_hints(f, main_chunks[2], app);
}

fn draw_info_section(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60), // Device info
            Constraint::Percentage(40), // Instructions
        ])
        .split(area);

    // Device info
    let info_text = if app.backend.has_identity() {
        let device_count = app.backend.list_devices().map(|d| d.len()).unwrap_or(0);
        vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("Devices: ", Style::default().fg(Color::Cyan)),
                Span::raw(format!("{}", device_count)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Manage linked devices for this identity",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "No identity configured",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Create an identity first to manage devices",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    };

    let info = Paragraph::new(info_text)
        .block(Block::default().borders(Borders::ALL).title("Device Info"));
    f.render_widget(info, chunks[0]);

    // Instructions
    let instructions = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("[l]", Style::default().fg(Color::Yellow)),
            Span::raw(" Generate link code"),
        ]),
        Line::from(vec![
            Span::styled("[r]", Style::default().fg(Color::Yellow)),
            Span::raw(" Revoke selected device"),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).title("Actions"));
    f.render_widget(instructions, chunks[1]);
}

fn draw_device_list(f: &mut Frame, area: Rect, app: &App) {
    let devices = app.backend.list_devices().unwrap_or_default();

    if devices.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "No devices linked to this identity",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::raw("Press [l] to generate a device link code")),
        ])
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Linked Devices"),
        );
        f.render_widget(empty, area);
        return;
    }

    let device_items: Vec<ListItem> = devices
        .iter()
        .enumerate()
        .map(|(idx, device)| {
            let is_selected = idx == app.selected_device;
            let prefix = if is_selected { "› " } else { "  " };

            let status_span = if device.is_current {
                Span::styled(" [this device]", Style::default().fg(Color::Green))
            } else if device.is_active {
                Span::styled(" [active]", Style::default().fg(Color::Blue))
            } else {
                Span::styled(" [revoked]", Style::default().fg(Color::Red))
            };

            let line = Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("{}. ", device.device_index + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(&device.device_name),
                status_span,
                Span::styled(
                    format!(
                        "  ({}...)",
                        &device.public_key_prefix[..8.min(device.public_key_prefix.len())]
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);

            let style = if is_selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let device_list = List::new(device_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Linked Devices"),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_widget(device_list, area);
}

fn draw_key_hints(f: &mut Frame, area: Rect, _app: &App) {
    let hints = Paragraph::new(Line::from(vec![
        Span::styled("j/↓", Style::default().fg(Color::Yellow)),
        Span::raw(" Down  "),
        Span::styled("k/↑", Style::default().fg(Color::Yellow)),
        Span::raw(" Up  "),
        Span::styled("l", Style::default().fg(Color::Yellow)),
        Span::raw(" Link  "),
        Span::styled("r", Style::default().fg(Color::Yellow)),
        Span::raw(" Revoke  "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" Back"),
    ]))
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::TOP));

    f.render_widget(hints, area);
}

/// Generate and display device link QR code
#[allow(dead_code)]
pub fn show_device_link_qr(app: &mut App) -> Option<String> {
    app.backend.generate_device_link().ok()
}

/// Render a QR code as ASCII art.
#[allow(dead_code)]
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
