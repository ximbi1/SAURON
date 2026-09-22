use crate::{
    app::state::{Document, Mode, State},
    command::{Action, Keymap},
    kube::discovery::Resource,
    resources::{age_text, health::Severity},
};
use chrono::Utc;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Row, Table, Wrap},
};

/// The single source of truth for which theme names `Theme::named` gives a
/// real (non-fallback) look for. `Config`'s own startup fail-safe check and
/// the live `:theme` command both validate against this, never a private
/// copy of the list.
pub const KNOWN_THEMES: &[&str] = &["ember", "light", "mono"];

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
        self.role(ThemeRole::from(s))
    }
    /// M10.7: the one place any renderer should ask for presentational
    /// color -- a closed role, never an arbitrary `Style` built ad hoc
    /// at the call site. Centralizes presentation; carries no semantic
    /// meaning of its own (a role does not decide WHAT is
    /// healthy/denied/selected, only how that fact is colored once
    /// something else has already decided it).
    pub fn role(self, role: ThemeRole) -> Color {
        match role {
            ThemeRole::Normal => self.good,
            ThemeRole::Muted => self.muted,
            ThemeRole::Selected => self.accent,
            ThemeRole::Warning => self.warning,
            ThemeRole::Critical => self.critical,
            ThemeRole::Unknown => self.muted,
        }
    }
}

/// M10.7: a small closed set every renderer asks for by role, not an
/// arbitrary `Style` -- generalizes `Theme::severity()`'s own pre-M10.7
/// pattern (`Normal`/`Unknown`/`Warning`/`Critical` line up with
/// `Severity`'s own variants -- `Healthy` is spelled `Normal` here
/// because a role is a presentation concept, not a health concept) plus
/// two new roles this slice's own UI needs: `Muted` (de-emphasized/
/// secondary text) and `Selected` (cursor/multi-select marker).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeRole {
    Normal,
    Muted,
    Selected,
    Warning,
    Critical,
    Unknown,
}
impl From<Severity> for ThemeRole {
    fn from(s: Severity) -> Self {
        match s {
            Severity::Healthy => ThemeRole::Normal,
            Severity::Unknown => ThemeRole::Unknown,
            Severity::Warning => ThemeRole::Warning,
            Severity::Critical => ThemeRole::Critical,
        }
    }
}

/// M10.7: the non-color distinguishing channel -- reconnaissance found
/// the "mono" theme already exists (every `Color` is `Color::Reset`,
/// i.e. zero color information at all) but table row severity was
/// color-only (`Style::default().fg(theme.severity(...))`  with no
/// accompanying marker), the concrete gap this slice closes. A
/// monochrome/no-color terminal must still be able to tell healthy from
/// warning/critical/unknown from this glyph alone.
pub fn severity_glyph(s: Severity) -> char {
    match s {
        Severity::Healthy => ' ',
        Severity::Warning => '!',
        Severity::Critical => '✗',
        Severity::Unknown => '?',
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
    let fullscreen =
        matches!(&state.mode, Mode::Document(doc) | Mode::Search(doc, _) if doc.fullscreen);
    // M13.5: a taller, informational header (bordered box: Context/
    // Cluster/Namespace/Resource/Objects) replaces the plain 2-line
    // heading, but only on a genuinely large terminal -- deliberately
    // well above every existing acceptance script's own standard
    // terminal size (180x40, used throughout M3-M12's own `accept-*.py`
    // scripts, several of which assert the plain "ctx:X" breadcrumb text
    // verbatim). Raising this threshold above that established size,
    // rather than reusing `show_banner`'s own lower one below, keeps
    // every existing script's assertions -- and every real user's
    // experience at that already-common size -- completely unchanged;
    // only a terminal meaningfully larger than that convention (like the
    // user's own real 237x61 terminal this slice was verified against)
    // gets the fuller panel.
    let expanded_header = !fullscreen && area.height >= 45 && area.width >= 200;
    let parts = Layout::vertical([
        Constraint::Length(if fullscreen {
            0
        } else if expanded_header {
            7
        } else if area.height < 16 {
            2
        } else {
            4
        }),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    // Short, canonical breadcrumb for the current scope: ctx:X › ns:Y › resource, or
    // ctx:X › resource with no ns segment for a cluster-scoped resource. Always the
    // resolved GVK's qualified name, never the raw alias the user may have typed.
    let cluster_scoped = state.resource.as_ref().is_some_and(|r| !r.namespaced);
    let resource_label = state
        .resource
        .as_ref()
        .map(Resource::qualified)
        .unwrap_or_else(|| state.query.resource.clone());
    // M13.3: the decorative banner sits to the right of the header text.
    // Deliberately its OWN, lower threshold -- unchanged since M13.3's own
    // live-verified acceptance -- independent of `expanded_header`'s much
    // higher one above, so a terminal that shows the banner but not yet
    // the full box (e.g. every existing accept-*.py script's own 180x40)
    // keeps behaving exactly as M13.3 already shipped.
    let show_banner =
        !fullscreen && area.height >= 16 && area.width >= 100 && state.settings.banner;
    let (info_area, banner_area) = if show_banner {
        let cols = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(crate::brand::BANNER_WIDTH + 2),
        ])
        .split(parts[0]);
        (cols[0], Some(cols[1]))
    } else {
        (parts[0], None)
    };
    if let Some(banner_area) = banner_area {
        // Matches the website's own `.terminal-eye` styling verbatim: the
        // shield outline (top/bottom lines) in the "red" role, the eye
        // mark's own line in "amber", and the "SAUR-ON" label muted --
        // never one flat color for the whole mark.
        frame.render_widget(
            Paragraph::new(
                crate::brand::BANNER
                    .iter()
                    .enumerate()
                    .map(|(i, line)| {
                        let color = match i {
                            0 | 2 => theme.critical,
                            1 => theme.warning,
                            _ => theme.muted,
                        };
                        Line::from(Span::styled(*line, Style::default().fg(color)))
                    })
                    .collect::<Vec<_>>(),
            )
            .alignment(ratatui::layout::Alignment::Center),
            banner_area,
        );
    }
    if expanded_header {
        let ns_label = state.query.namespace.as_deref().unwrap_or("<all>");
        let server = if state.server.is_empty() {
            "Not connected"
        } else {
            state.server.as_str()
        };
        let objects = if state.synced {
            format!("{} synchronized", state.rows.len())
        } else {
            format!("{} (syncing…)", state.rows.len())
        };
        let field = |label: &'static str, value: String| {
            Line::from(vec![
                Span::styled(format!("{label:<12}"), Style::default().fg(theme.muted)),
                Span::styled(value, Style::default().fg(theme.accent)),
            ])
        };
        let context_value = if state.settings.readonly {
            format!("{} [read-only]", state.context)
        } else {
            state.context.clone()
        };
        frame.render_widget(
            Paragraph::new(vec![
                field("Context:", context_value),
                field("Cluster:", server.to_string()),
                field("Namespace:", ns_label.to_string()),
                field("Resource:", resource_label.clone()),
                field("Objects:", objects),
            ])
            .block(
                Block::bordered()
                    .title(format!(" {} ", crate::brand::BINARY))
                    .border_style(Style::default().fg(theme.muted))
                    .padding(ratatui::widgets::Padding::horizontal(1)),
            ),
            info_area,
        );
    } else {
        let heading = format!(
            "{} {}  ·  {} · PF {}",
            crate::brand::MARK,
            crate::brand::NAME,
            if state.settings.readonly {
                "READ ONLY"
            } else {
                "OPERATIONAL (exec/shell/attach/forward enabled)"
            },
            state.forward_count,
        );
        let scope = if cluster_scoped {
            format!("ctx:{} › {resource_label}", state.context)
        } else {
            let ns = state.query.namespace.as_deref().unwrap_or("*");
            format!("ctx:{} › ns:{ns} › {resource_label}", state.context)
        };
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
            info_area,
        );
    }
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
                let mut cells = columns
                    .iter()
                    .map(|column| {
                        crate::safety::text(&if column == "AGE" {
                            age_text(o.age(now))
                        } else {
                            state.cell(o, column, now).unwrap_or_else(|| "-".into())
                        })
                        .replace(['\n', '\t'], " ")
                    })
                    .collect::<Vec<_>>();
                // M10.1: a distinct marker from the cursor's own `› `
                // highlight_symbol below -- cursor focus and bulk
                // multi-select membership are independent and must both
                // stay visible at once, never collapsed into one glyph.
                // M10.7: the severity glyph right after it is the non-
                // color channel -- on the "mono" theme (all colors reset)
                // this is the ONLY way severity is distinguishable at all.
                if let Some(first) = cells.first_mut() {
                    let selected = if state.selection.contains(&o.uid) {
                        '✓'
                    } else {
                        ' '
                    };
                    first.insert_str(
                        0,
                        &format!("{selected}{} ", severity_glyph(o.health.severity)),
                    );
                }
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
                " {} [{} / {}; ?{}] · {} {} {} ",
                state
                    .resource
                    .as_ref()
                    .map(Resource::qualified)
                    .unwrap_or_else(|| state.query.resource.clone()),
                state.rows.len(),
                state.store.objects.len(),
                state.filter_unknown,
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
                    Paragraph::new(if state.synced && state.filter_unknown > 0 {
                        "No TRUE matches; some rows UNKNOWN (missing or invalid fields)"
                    } else if state.synced {
                        "No matching resources"
                    } else {
                        "Waiting for resource list…"
                    }),
                    empty,
                );
            }
        }
    }
    let query_status = format!(
        "{} · unknown excluded: {} · -l {} · -f {} · /{}",
        state.status,
        state.filter_unknown,
        state.query.labels.as_deref().unwrap_or("<none>"),
        state.query.fields.as_deref().unwrap_or("<none>"),
        state.filter_text
    );
    let document_status = match &state.mode {
        Mode::Document(doc) | Mode::Search(doc, _) => Some(doc.status()),
        _ => None,
    };
    // M10.9: `startup_warning` is the lowest priority -- a real-time
    // input/watch error always takes the status line when present, but
    // when nothing more urgent is happening, the config warning stays
    // visible for the rest of the session instead of just disappearing
    // the moment the list first syncs (see `State::new`'s own doc
    // comment for the live finding that required this).
    let error = state
        .input_error
        .as_ref()
        .or(state.error.as_ref())
        .or(state.startup_warning.as_ref());
    let status = error.or(document_status.as_ref()).unwrap_or(&query_status);
    frame.render_widget(
        Paragraph::new(crate::safety::text(status)).style(Style::default().fg(
            if error.is_some() {
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
        Mode::Document(doc) if doc.forward_manager => [
            (Action::Back, "return"),
            (Action::StopForward, "stop ID"),
            (Action::Refresh, "refresh"),
            (Action::Help, "help"),
        ]
        .into_iter()
        .filter_map(|(action, label)| {
            state
                .keymap
                .primary_key(action)
                .map(|key| format!("{key} {label}"))
        })
        .collect::<Vec<_>>()
        .join("   "),
        Mode::Document(doc) if doc.session.is_some() => [
            (Action::Back, "return"),
            (Action::PauseLogs, "pause/resume"),
            (Action::ClearLogs, "clear"),
            (Action::Filter, "search"),
            (Action::FilterLogs, "matching lines"),
            (Action::Refresh, "restart"),
        ]
        .into_iter()
        .filter_map(|(action, label)| {
            state
                .keymap
                .primary_key(action)
                .map(|key| format!("{key} {label}"))
        })
        .collect::<Vec<_>>()
        .join("   "),
        _ => hint_bar(&state.keymap, matches!(state.mode, Mode::Document(_))),
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
/// The idle-mode hint bar, built from whatever the effective keymap actually binds --
/// never a hardcoded string of default keys, which would silently lie once a user
/// overrides a binding in config. Actions with no bound key are simply omitted.
fn hint_bar(keymap: &Keymap, document: bool) -> String {
    let hint = |action: Action, label: &str| {
        keymap
            .primary_key(action)
            .map(|key| format!("{key} {label}"))
    };
    let parts = if document {
        [
            hint(Action::Back, "return"),
            hint(Action::Filter, "search"),
            hint(Action::SearchNext, "next"),
            hint(Action::Wrap, "wrap"),
            hint(Action::Refresh, "refresh"),
            hint(Action::Help, "help"),
        ]
    } else {
        [
            hint(Action::Palette, "commands"),
            hint(Action::Filter, "filter/search"),
            hint(Action::Explain, "explain"),
            hint(Action::Yaml, "YAML"),
            hint(Action::Logs, "logs"),
            hint(Action::Help, "help"),
        ]
    };
    format!(
        " {} ",
        parts.into_iter().flatten().collect::<Vec<_>>().join("   ")
    )
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
    doc.layout(area.width.saturating_sub(2) as usize, height);
    let block = Block::bordered().title(crate::safety::text(&doc.title));
    if let crate::app::document::Freshness::Error(error) = &doc.freshness {
        frame.render_widget(
            Paragraph::new(format!(
                "NOT CURRENT\n{error}\nReturn to the table to select a current object."
            ))
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(theme.critical))
            .block(block),
            area,
        );
        return;
    }
    let target_line = doc.selected_target_line();
    let text: Vec<Line> = doc
        .visible_line_numbers()
        .zip(doc.visible_lines())
        .map(|(line_no, line)| {
            let mut spans = Vec::new();
            let mut start = 0;
            if let Some(regex) = &doc.search_regex {
                for m in regex.find_iter(line) {
                    spans.push(Span::raw(&line[start..m.start()]));
                    spans.push(Span::styled(
                        m.as_str(),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::REVERSED),
                    ));
                    start = m.end();
                }
            }
            spans.push(Span::raw(&line[start..]));
            let mut rendered = Line::from(spans);
            if Some(line_no) == target_line {
                rendered = rendered.style(Style::default().add_modifier(Modifier::REVERSED));
            }
            rendered
        })
        .collect();
    let paragraph = Paragraph::new(text)
        .scroll((0, if doc.wrap { 0 } else { doc.horizontal }))
        .block(block);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn severity_glyph_is_distinct_for_every_variant_never_two_the_same() {
        let glyphs: Vec<char> = [
            Severity::Healthy,
            Severity::Warning,
            Severity::Critical,
            Severity::Unknown,
        ]
        .into_iter()
        .map(severity_glyph)
        .collect();
        let mut sorted = glyphs.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            glyphs.len(),
            "every severity must have its own distinct non-color marker: {glyphs:?}"
        );
    }
    #[test]
    fn theme_role_from_severity_preserves_the_exact_pre_m10_7_colors() {
        // The whole point of generalizing through ThemeRole is that the
        // ember theme's own rendered colors must not change -- this is
        // the same mapping `Theme::severity` used before M10.7, now
        // reached through `role()` instead of a private match.
        let theme = Theme::named("ember");
        assert_eq!(theme.severity(Severity::Healthy), theme.good);
        assert_eq!(theme.severity(Severity::Unknown), theme.muted);
        assert_eq!(theme.severity(Severity::Warning), theme.warning);
        assert_eq!(theme.severity(Severity::Critical), theme.critical);
    }
    #[test]
    fn mono_theme_still_has_a_non_color_channel() {
        // "mono" sets every Color to Reset -- role() colors carry zero
        // information there, so severity_glyph is the ONLY thing that
        // still distinguishes healthy from warning/critical/unknown.
        let mono = Theme::named("mono");
        assert_eq!(mono.role(ThemeRole::Normal), Color::Reset);
        assert_eq!(mono.role(ThemeRole::Critical), Color::Reset);
        assert_ne!(
            severity_glyph(Severity::Healthy),
            severity_glyph(Severity::Critical)
        );
    }
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
            );
            terminal
                .draw(|frame| render(frame, &mut state, &[]))
                .expect("render");
        }
    }

    #[test]
    fn startup_warning_actually_appears_in_the_rendered_status_line() {
        let settings = crate::config::Settings {
            theme: "not-a-real-theme".into(),
            ..crate::config::Settings::default()
        };
        let mut state = State::new(
            crate::kube::watch::Query {
                resource: "pods".into(),
                ..Default::default()
            },
            &settings,
        );
        assert!(state.startup_warning.is_some());
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(frame, &mut state, &[]))
            .expect("render");
        let content = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(
            content.contains("Unknown theme"),
            "the startup warning must actually be visible in the rendered frame, not just set on state"
        );
    }

    /// M8B.5: a real Drain preview/report is one of the longest documents
    /// in the app (Node target, cordon step, a full Pod plan, every
    /// safety-contract line, then a per-step composite result) -- proves
    /// it renders without panicking even at a genuinely cramped 32x9
    /// terminal, not just the wider sizes above.
    #[test]
    fn drain_preview_and_report_remain_usable_at_32_by_9() {
        use kube::core::ApiResource;
        let node_scope = crate::app::session::Scope {
            epoch: 1,
            request: 1,
            context: "kind-sauron-test".into(),
            cluster: "kind-sauron-test".into(),
            resource: "v1/nodes".into(),
            namespace: String::new(),
            name: "node-1".into(),
            uid: "node-uid".into(),
        };
        let node_resource = Resource {
            api: ApiResource {
                group: "".into(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: "Node".into(),
                plural: "nodes".into(),
            },
            namespaced: false,
            short_names: vec![],
            verbs: vec!["patch".into()],
        };
        let pod_resource = Resource {
            api: ApiResource {
                group: "".into(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: "Pod".into(),
                plural: "pods".into(),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec!["delete".into()],
        };
        let built = crate::mutation::workflow::cordon(node_scope, node_resource, 1)
            .expect("cordon supported for Node");
        let evaluation = crate::mutation::policy::evaluate(
            &crate::mutation::policy::PolicyContext {
                readonly: false,
                readonly_forced: false,
                cluster_verified_for_mutation: true,
                ..Default::default()
            },
            &built.intent,
        );
        let mut drain = crate::mutation::drain::DrainWorkflow::new(
            built.intent,
            built.payload,
            built.change,
            pod_resource,
            evaluation,
        );
        drain.preview = crate::mutation::drain::DrainPreview::Ready(
            (0..8)
                .map(|i| crate::mutation::drain::PlannedPod {
                    namespace: "default".into(),
                    name: format!("web-{i}"),
                    uid: format!("uid-{i}"),
                    exclusion: match i % 3 {
                        0 => None,
                        1 => Some(crate::mutation::drain::Exclusion::DaemonSetOwned),
                        _ => Some(crate::mutation::drain::Exclusion::LocalStorage),
                    },
                })
                .collect(),
        );
        let text = crate::mutation::view::drain_report(&drain);
        assert!(text.contains("PODS PLANNED FOR EVICTION"));
        let mut doc = Document::new("Drain: node-1".into(), text);
        doc.drain = Some(drain);
        let mut state = State::new(
            crate::kube::watch::Query {
                resource: "nodes".into(),
                ..Default::default()
            },
            &crate::config::Settings::default(),
        );
        state.mode = Mode::Document(doc);
        let backend = ratatui::backend::TestBackend::new(32, 9);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(frame, &mut state, &[]))
            .expect("render preview at 32x9");
        // Now the composite, post-commit report -- a distinct, usually
        // longer document (every step's outcome plus a summary line).
        if let Mode::Document(doc) = &mut state.mode {
            let drain = doc.drain.as_mut().expect("drain");
            drain.report = Some(crate::mutation::drain::DrainReport {
                cordon_outcome: crate::mutation::MutationOutcome::Committed,
                steps: (0..8)
                    .map(|i| crate::mutation::drain::DrainStep {
                        namespace: "default".into(),
                        name: format!("web-{i}"),
                        uid: format!("uid-{i}"),
                        outcome: match i % 3 {
                            0 => crate::mutation::drain::StepOutcome::Attempted(
                                crate::mutation::MutationOutcome::Committed,
                            ),
                            1 => crate::mutation::drain::StepOutcome::Excluded(
                                crate::mutation::drain::Exclusion::DaemonSetOwned,
                            ),
                            _ => crate::mutation::drain::StepOutcome::NotAttempted,
                        },
                        verification: None,
                    })
                    .collect(),
            });
            let text = crate::mutation::view::drain_report(drain);
            doc.replace(text);
        }
        terminal
            .draw(|frame| render(frame, &mut state, &[]))
            .expect("render report at 32x9");
    }
}
