use ratatui::{
    layout::{Constraint, Direction, Layout, Margin},
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use textwrap::wrap;

use crate::app::{App, AppMode};
use crate::scaffold::ScaffoldChoice;
use crate::theme::{text_accent, text_fade, text_primary, text_secondary, text_warning};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let size = frame.size();

    let inner = size.inner(&Margin { vertical: 3, horizontal: 3 });
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(inner);

    let status_area = chunks[0];
    let main_area = chunks[1];
    let input_area = chunks[2];

    app.aura.render(frame, size, app.voice_intensity);

    frame.render_widget(Clear, status_area);
    let status = format!("mode={:?}", app.mode);
    let status_widget = Paragraph::new(status)
        .block(Block::default().borders(Borders::ALL).title("RIPL"))
        .style(Style::default().fg(text_secondary()))
        .wrap(Wrap { trim: true });
    frame.render_widget(status_widget, status_area);

    let wrap_width = main_area.width.saturating_sub(2) as usize;
    let wrapped_lines = wrap_messages(&app.messages, wrap_width);
    let history_lines = wrapped_lines.len();
    let visible_lines = main_area.height.saturating_sub(2) as usize;
    let max_offset = history_lines.saturating_sub(visible_lines);
    let scroll = max_offset.saturating_sub(app.history_offset.min(max_offset)) as u16;
    let history = wrapped_lines.join("\n");
    let history_widget = Paragraph::new(history)
        .block(Block::default().borders(Borders::ALL).title("Thread"))
        .style(Style::default().fg(text_primary()))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(Clear, main_area);
    frame.render_widget(history_widget, main_area);

    frame.render_widget(Clear, input_area);
    let input_widget = Paragraph::new(input_line(app))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(match app.mode {
                    AppMode::Setup => "Setup",
                    AppMode::Ready => "Ready",
                    AppMode::Pending => "Pending",
                    AppMode::Streaming => "Streaming",
                })
                .border_style(Style::default().fg(text_accent())),
        )
        .style(Style::default().fg(text_primary()));
    frame.render_widget(input_widget, input_area);

    let x = input_area.x + 1 + app.input.chars().count() as u16;
    let y = input_area.y + 1;
    frame.set_cursor(x, y);

    if let Some(selected) = app.scaffold_prompt {
        draw_scaffold_prompt(frame, selected);
    }
}

fn input_line(app: &App) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(ratatui::text::Span::styled(
        app.input.clone(),
        Style::default().fg(text_primary()),
    ));
    if let Some((text, color)) = stt_status_tag(app) {
        spans.push(ratatui::text::Span::styled(
            format!(" {}", text),
            Style::default().fg(color),
        ));
    }
    Line::from(spans)
}

fn stt_status_tag(app: &App) -> Option<(String, ratatui::style::Color)> {
    if app.stt_error.is_some() {
        return Some(("[ stt error ]".to_string(), text_warning()));
    }
    if app.tts_error.is_some() {
        return Some(("[ tts error ]".to_string(), text_warning()));
    }
    if app.stt_recording {
        return Some(("[ ● rec ]".to_string(), text_accent()));
    }
    if app.stt_transcribing {
        return Some(("[ … ]".to_string(), text_fade(0.7)));
    }
    None
}

fn wrap_messages(messages: &[String], width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for msg in messages {
        for line in msg.split('\n') {
            if width == 0 {
                out.push(line.to_string());
                continue;
            }
            let wrapped = wrap(line, width);
            if wrapped.is_empty() {
                out.push(String::new());
            } else {
                for w in wrapped {
                    out.push(w.into_owned());
                }
            }
        }
    }
    out
}

fn draw_scaffold_prompt(frame: &mut Frame, selected: ScaffoldChoice) {
    let area = frame.size();
    let width = 54.min(area.width.saturating_sub(4));
    let height = 9.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = ratatui::layout::Rect { x, y, width, height };

    frame.render_widget(Clear, rect);
    let title = "Bootstrap scaffold?";
    let mut lines = Vec::new();
    lines.push("Missing README.md / .claude/CLAUDE.md / skills/README.md".to_string());
    lines.push("Choose:".to_string());
    lines.push(option_line("Leave", 'L', selected == ScaffoldChoice::Leave));
    lines.push(option_line("Append", 'A', selected == ScaffoldChoice::Append));
    lines.push(option_line("Overwrite", 'O', selected == ScaffoldChoice::Overwrite));
    lines.push("Enter to confirm, Esc = Leave".to_string());
    let body = lines.join("\n");

    let block = Paragraph::new(body)
        .block(Block::default().borders(Borders::ALL).title(title))
        .style(Style::default().fg(text_primary()));
    frame.render_widget(block, rect);

}

fn option_line(label: &str, key: char, selected: bool) -> String {
    if selected {
        format!("> [{key}] {label}")
    } else {
        format!("  [{key}] {label}")
    }
}
