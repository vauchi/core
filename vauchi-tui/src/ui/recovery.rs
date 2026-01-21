//! Recovery Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

/// Draw the recovery screen.
pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9), // Status
            Constraint::Length(8), // Actions
            Constraint::Min(0),    // Info
            Constraint::Length(3), // Key hints
        ])
        .split(area);

    // Recovery status - dynamic from backend
    draw_status_section(f, chunks[0], app);

    // Recovery actions
    draw_actions_section(f, chunks[1], app);

    // Recovery info
    draw_info_section(f, chunks[2]);

    // Key hints
    draw_key_hints(f, chunks[3]);
}

fn draw_status_section(f: &mut Frame, area: Rect, app: &App) {
    let status_result = app.backend.get_recovery_status();

    let status_text = match status_result {
        Ok(status) => {
            if status.has_active_claim {
                let progress = if status.required_vouchers > 0 {
                    (status.voucher_count as f64 / status.required_vouchers as f64).min(1.0)
                } else {
                    0.0
                };

                let progress_color = if progress >= 1.0 {
                    Color::Green
                } else if progress >= 0.5 {
                    Color::Yellow
                } else {
                    Color::Red
                };

                let expiry_text = status
                    .claim_expires
                    .clone()
                    .unwrap_or_else(|| "No expiration".to_string());

                vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "Recovery Claim Active",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("  Vouchers: "),
                        Span::styled(
                            format!("{}/{}", status.voucher_count, status.required_vouchers),
                            Style::default().fg(progress_color),
                        ),
                    ]),
                    Line::from(vec![
                        Span::raw("  Expires: "),
                        Span::styled(expiry_text, Style::default().fg(Color::DarkGray)),
                    ]),
                    Line::from(""),
                ]
            } else {
                vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "Recovery Status",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "  No active recovery claim",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "  Press [c] to create a recovery claim if needed",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(""),
                ]
            }
        }
        Err(_) => {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Recovery Status",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Unable to load recovery status",
                    Style::default().fg(Color::Red),
                )),
                Line::from(""),
            ]
        }
    };

    let status =
        Paragraph::new(status_text).block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(status, area);
}

fn draw_actions_section(f: &mut Frame, area: Rect, app: &App) {
    let has_identity = app.backend.has_identity();

    let actions_text = if has_identity {
        vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  [c]", Style::default().fg(Color::Yellow)),
                Span::raw(" Create recovery claim"),
            ]),
            Line::from(vec![
                Span::styled("  [v]", Style::default().fg(Color::Yellow)),
                Span::raw(" Vouch for a contact's recovery"),
            ]),
            Line::from(vec![
                Span::styled("  [s]", Style::default().fg(Color::Yellow)),
                Span::raw(" Check recovery status"),
            ]),
            Line::from(""),
        ]
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No identity configured",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Create an identity first",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
        ]
    };

    let actions =
        Paragraph::new(actions_text).block(Block::default().borders(Borders::ALL).title("Actions"));
    f.render_widget(actions, area);
}

fn draw_info_section(f: &mut Frame, area: Rect) {
    let info_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "How Recovery Works",
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from("  1. Lost your device? Create a new identity on a new device"),
        Line::from("  2. Generate a recovery claim with your OLD public key"),
        Line::from("  3. Ask 3+ contacts to vouch for you in person"),
        Line::from("  4. Collect vouchers to prove you're the same person"),
        Line::from("  5. Share your recovery proof with all contacts"),
        Line::from(""),
        Line::from(Span::styled(
            "Note: Recovery requires mutual verification in person.",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            "Use CLI for detailed recovery workflow: vauchi recovery --help",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
    ];

    let info = Paragraph::new(info_text)
        .block(Block::default().borders(Borders::ALL).title("Information"));
    f.render_widget(info, area);
}

fn draw_key_hints(f: &mut Frame, area: Rect) {
    let hints = Paragraph::new(Line::from(vec![
        Span::styled("c", Style::default().fg(Color::Yellow)),
        Span::raw(" Claim  "),
        Span::styled("v", Style::default().fg(Color::Yellow)),
        Span::raw(" Vouch  "),
        Span::styled("s", Style::default().fg(Color::Yellow)),
        Span::raw(" Status  "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" Back"),
    ]))
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::TOP));

    f.render_widget(hints, area);
}
