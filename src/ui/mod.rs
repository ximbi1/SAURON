use crate::{
    app::state::{Document, Mode, State},
    resources::{age_text, health::Severity},
};
use chrono::Utc;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap},
};

#[derive(Clone, Copy)]
pub struct Theme {
    pub foreground: Color,
    pub background: Color,
    pub accent: Color,
    pub good: Color,
    pub warning: Color,
    pub critical: Color,
    pub muted: Color,
}
impl Theme {
    pub fn named(name: &str) -> Self {
        match name {
            "light" => Self {
                foreground: Color::Rgb(30, 36, 44),
                background: Color::Rgb(247, 247, 242),
                accent: Color::Rgb(140, 60, 0),
                good: Color::Rgb(0, 90, 65),
                warning: Color::Rgb(150, 90, 0),
                critical: Color::Rgb(180, 30, 40),
                muted: Color::Rgb(90, 95, 105),
            },
            "mono" => Self {
                foreground: Color::Reset,
                background: Color::Reset,
                accent: Color::Reset,
                good: Color::Reset,
                warning: Color::Reset,
                critical: Color::Reset,
                muted: Color::Reset,
            },
            _ => Self {
                foreground: Color::Rgb(218, 224, 232),
                background: Color::Rgb(16, 20, 27),
                accent: Color::Rgb(240, 158, 69),
                good: Color::Rgb(113, 198, 161),
                warning: Color::Rgb(233, 190, 91),
                critical: Color::Rgb(244, 112, 112),
                muted: Color::Rgb(140, 150, 166),
            },
        }
    }
    fn severity(self, s: Severity) -> Color {
        match s {
            Severity::Healthy => self.good,
            Severity::Unknown => self.muted,
            Severity::Warning => self.warning,
            Severity::Critical => self.critical,
        }
    }
}

pub fn render(frame: &mut Frame, state: &mut State, suggestions: &[String]) {
    let theme = Theme::named(&state.settings.theme);
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().fg(theme.foreground).bg(theme.background)),
        area,
    );
    if area.width < 20 || area.height < 5 {
        frame.render_widget(Paragraph::new("Terminal too small\nCtrl-C quits"), area);
        return;
    }
    if let Mode::Document(doc) = &mut state.mode
        && doc.fullscreen
    {
        render_document(frame, area, doc, theme);
        return;
    }
    let parts = Layout::vertical([
        Constraint::Length(if area.height < 16 { 2 } else { 4 }),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    let heading = format!(
        "{} {}  ·  {}  ·  READ ONLY",
        crate::brand::MARK,
        crate::brand::NAME,
        crate::safety::text(&state.context)
    );
    let scope = format!(
        "Namespace: {}   Resource: {}",
        state.query.namespace.as_deref().unwrap_or("<all>"),
        state.query.resource
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                heading,
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(crate::safety::text(&scope)),
        ]),
        parts[0],
    );
    state.page_size = parts[1].height.saturating_sub(3) as usize;
    let columns = state.columns();
    match &mut state.mode {
        Mode::Document(doc) | Mode::Search(doc, _) => render_document(frame, parts[1], doc, theme),
        Mode::Picker(picker) => render_picker(frame, parts[1], picker, theme),
        Mode::Loading => frame.render_widget(
            Paragraph::new(
                "Gathering fresh evidence…\nEsc cancels; Kubernetes work runs in the background.",
            )
            .block(Block::bordered().title("Evidence")),
            parts[1],
        ),
        _ => {
            let now = Utc::now();
            let rows = state.rows.iter().map(|o| {
                let cells = columns
                    .iter()
                    .map(|column| {
                        crate::safety::text(&if column == "AGE" {
                            age_text(o.age(now))
                        } else {
                            o.field(column, now).unwrap_or_else(|| "-".into())
                        })
                        .replace(['\n', '\t'], " ")
                    })
                    .collect::<Vec<_>>();
                Row::new(cells).style(Style::default().fg(theme.severity(o.health.severity)))
            });
            let widths: Vec<_> = columns
                .iter()
                .map(|c| match c.as_str() {
                    "NAME" => Constraint::Min(18),
                    "NAMESPACE" => Constraint::Length(16),
                    "STATUS" => Constraint::Length(26),
                    "READY" | "AGE" => Constraint::Length(7),
                    "RESTARTS" => Constraint::Length(8),
                    _ => Constraint::Length(16),
                })
                .collect();
            let title = format!(
                " {} [{} / {}] · {} {} {} ",
                state.query.resource,
                state.rows.len(),
                state.store.objects.len(),
                state.sort,
                if state.descending { "↓" } else { "↑" },
                if state.store.incomplete {
                    "PARTIAL: cache limit"
                } else if !state.synced {
                    "loading/stale"
                } else {
                    "list synchronized"
                }
            );
            let table = Table::new(rows, widths)
                .header(
                    Row::new(columns).style(
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .block(Block::bordered().title(title))
                .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                .highlight_symbol("› ");
            frame.render_stateful_widget(table, parts[1], &mut state.table);
            if state.rows.is_empty() {
                let empty = Rect {
                    x: parts[1].x + 2,
                    y: parts[1].y + 3,
                    width: parts[1].width.saturating_sub(4),
                    height: 1,
                };
                frame.render_widget(
                    Paragraph::new(if state.synced {
                        "No matching resources"
                    } else {
                        "Waiting for resource list…"
                    }),
                    empty,
                );
            }
        }
    }
    let status = state.error.as_ref().unwrap_or(&state.status);
    frame.render_widget(
        Paragraph::new(crate::safety::text(status)).style(Style::default().fg(
            if state.error.is_some() {
                theme.critical
            } else {
                theme.muted
            },
        )),
        parts[2],
    );
    let prompt = match &state.mode {
        Mode::Command(text) => format!(":{text}▏"),
        Mode::Filter(text) => format!("/{text}▏"),
        Mode::Search(_, text) => format!("search /{text}▏"),
        Mode::Picker(_) => " ↑↓ move   enter switch   esc cancel ".into(),
        _ => " : commands   / filter/search   X explain   y YAML   l logs   ? help ".into(),
    };
    frame.render_widget(
        Paragraph::new(prompt).style(Style::default().fg(theme.accent)),
        parts[3],
    );
    if matches!(state.mode, Mode::Command(_)) && !suggestions.is_empty() {
        let height = (suggestions.len() as u16 + 2).min(parts[1].height);
        let popup = Rect {
            x: parts[1].x,
            y: parts[1].bottom().saturating_sub(height),
            width: parts[1].width.min(60),
            height,
        };
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(suggestions.join("\n"))
                .style(Style::default().fg(theme.foreground).bg(theme.background))
                .block(Block::bordered().title("Commands / resources · Tab completes")),
            popup,
        );
    }
}
fn render_picker(frame: &mut Frame, area: Rect, picker: &crate::app::state::Picker, theme: Theme) {
    let rows = picker.items.iter().enumerate().map(|(i, item)| {
        let marker = if Some(i) == picker.active {
            "● "
        } else {
            "  "
        };
        Row::new([format!("{marker}{item}")]).style(if i == picker.cursor {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(theme.foreground)
        })
    });
    let table = Table::new(rows, [Constraint::Min(20)])
        .block(Block::bordered().title(picker.title.as_str()));
    frame.render_widget(table, area);
}
fn render_document(frame: &mut Frame, area: Rect, doc: &mut Document, theme: Theme) {
    let height = area.height.saturating_sub(2) as usize;
    if doc.follow {
        doc.scroll = doc.lines.len().saturating_sub(height);
    }
    let text: Vec<Line> = doc
        .lines
        .iter()
        .map(|line| {
            Line::from(Span::styled(
                line,
                Style::default().fg(
                    if !doc.search.is_empty()
                        && line.to_lowercase().contains(&doc.search.to_lowercase())
                    {
                        theme.accent
                    } else {
                        theme.foreground
                    },
                ),
            ))
        })
        .collect();
    let mut paragraph =
        Paragraph::new(text).scroll((doc.scroll.min(u16::MAX as usize) as u16, doc.horizontal));
    paragraph = paragraph.block(
        Block::default()
            .borders(if doc.fullscreen {
                Borders::TOP
            } else {
                Borders::ALL
            })
            .title(crate::safety::text(&doc.title)),
    );
    if doc.wrap {
        paragraph = paragraph.wrap(Wrap { trim: false });
    }
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn narrow_and_empty_terminal_do_not_panic() {
        for (w, h) in [(0, 0), (12, 3), (40, 12), (100, 30)] {
            let backend = ratatui::backend::TestBackend::new(w, h);
            let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
            let mut state = State::new(
                crate::kube::watch::Query {
                    resource: "pods".into(),
                    ..Default::default()
                },
                &crate::config::Settings::default(),
            )
            .expect("state");
            terminal
                .draw(|frame| render(frame, &mut state, &[]))
                .expect("render");
        }
    }
}
