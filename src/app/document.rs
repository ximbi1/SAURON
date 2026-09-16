//! One document model for YAML, describe, evidence, static help and logs.
use crate::{command::Action, kube::discovery::Resource, resources::SharedObject};
use chrono::{DateTime, Utc};
use regex::{Regex, RegexBuilder};
use std::{collections::VecDeque, ops::Range};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_LINES: usize = 5000;
const MAX_VISUAL: usize = 262_144;
const MAX_MATCHES: usize = 10_000;

#[derive(Clone)]
pub struct Source {
    pub resource: Resource,
    pub selected: SharedObject,
    pub action: Action,
    /// Events-only display filter (Warning severity); ignored by every other action.
    /// Lives on the refresh source, not a separate Document field, so toggling it goes
    /// through the exact same re-fetch-and-UID-check path as a manual Refresh.
    pub warning_only: bool,
}
#[derive(Clone, Debug)]
pub enum Freshness {
    Local,
    Snapshot(DateTime<Utc>),
    Refreshing,
    Error(String),
}
#[derive(Clone)]
struct Segment {
    line: usize,
    bytes: Range<usize>,
}

pub struct Document {
    pub title: String,
    pub lines: VecDeque<String>,
    pub scroll: usize,
    pub horizontal: u16,
    pub wrap: bool,
    pub fullscreen: bool,
    pub search: String,
    pub streaming: bool,
    pub session: Option<super::session::SessionId>,
    pub session_state: Option<super::session::State>,
    pub log_request: Option<crate::kube::logs::Request>,
    pub exec_request: Option<crate::kube::exec::Request>,
    pub source_errors: Vec<String>,
    pub filter_matches: bool,
    pub evicted: u64,
    pub follow: bool,
    pub source: Option<Source>,
    pub freshness: Freshness,
    pub truncated: bool,
    pub page_size: usize,
    pub search_regex: Option<Regex>,
    bytes: usize,
    revision: u64,
    layout_key: Option<(u64, usize, bool)>,
    visual: Vec<Segment>,
    hits: Vec<(usize, usize)>,
    hit: Option<usize>,
    width: usize,
    max_width: usize,
    layout_partial: bool,
    matches_partial: bool,
    search_dirty: bool,
    pending_evicted: usize,
}
impl Document {
    pub fn new(title: String, text: String) -> Self {
        let mut doc = Self {
            title: crate::safety::text(&title),
            lines: VecDeque::new(),
            scroll: 0,
            horizontal: 0,
            wrap: true,
            fullscreen: false,
            search: String::new(),
            streaming: false,
            session: None,
            session_state: None,
            log_request: None,
            exec_request: None,
            source_errors: vec![],
            filter_matches: false,
            evicted: 0,
            follow: false,
            source: None,
            freshness: Freshness::Local,
            truncated: false,
            page_size: 20,
            search_regex: None,
            bytes: 0,
            revision: 0,
            layout_key: None,
            visual: vec![],
            hits: vec![],
            hit: None,
            width: 80,
            max_width: 0,
            layout_partial: false,
            matches_partial: false,
            search_dirty: false,
            pending_evicted: 0,
        };
        doc.replace(text);
        doc
    }
    pub fn replace(&mut self, text: String) {
        let anchor = self.anchor();
        let previous = self.lines.get(anchor.0).cloned();
        self.lines.clear();
        self.bytes = 0;
        self.truncated = false;
        self.evicted = 0;
        self.pending_evicted = 0;
        for line in text.lines() {
            let line = crate::safety::text(line).replace('\t', "    ");
            if self.lines.len() >= MAX_LINES || self.bytes + line.len() > MAX_BYTES {
                self.truncated = true;
                break;
            }
            self.bytes += line.len();
            self.lines.push_back(line);
        }
        // Position is safe only if the old anchor line still exists. Search nearest
        // occurrence for shifted YAML, otherwise start at the top and do not guess.
        let retained = previous.as_ref().and_then(|old| {
            self.lines
                .iter()
                .enumerate()
                .filter(|(_, line)| *line == old)
                .min_by_key(|(i, _)| i.abs_diff(anchor.0))
                .map(|(i, _)| (i, anchor.1))
        });
        self.revision += 1;
        self.layout_key = None;
        self.visual.clear();
        self.scroll = 0;
        self.layout(self.width, self.page_size);
        if let Some(anchor) = retained {
            self.go_to(anchor);
        }
        self.index_search();
    }
    pub fn append(&mut self, line: String) {
        let line = crate::safety::text(&line).replace('\t', "    ");
        self.bytes += line.len();
        self.lines.push_back(line);
        while self.lines.len() > MAX_LINES || self.bytes > MAX_BYTES {
            if let Some(old) = self.lines.pop_front() {
                self.bytes = self.bytes.saturating_sub(old.len());
                self.truncated = true;
                self.evicted = self.evicted.saturating_add(1);
                self.pending_evicted = self.pending_evicted.saturating_add(1);
            } else {
                break;
            }
        }
        self.revision += 1;
        self.search_dirty = true;
    }
    fn anchor(&self) -> (usize, usize) {
        self.visual
            .get(self.scroll)
            .map(|v| (v.line, v.bytes.start))
            .unwrap_or((0, 0))
    }
    fn go_to(&mut self, anchor: (usize, usize)) {
        self.scroll = self
            .visual
            .iter()
            .position(|v| v.line == anchor.0 && (v.bytes.contains(&anchor.1) || v.bytes.is_empty()))
            .unwrap_or(0);
    }
    pub fn layout(&mut self, width: usize, height: usize) {
        if self.search_dirty {
            self.index_search();
        }
        self.page_size = height.max(1);
        self.width = width.max(1);
        let key = (self.revision, self.width, self.wrap);
        if self.layout_key != Some(key) {
            let mut anchor = self.anchor();
            anchor.0 = anchor.0.saturating_sub(self.pending_evicted);
            self.pending_evicted = 0;
            self.visual.clear();
            self.max_width = 0;
            self.layout_partial = false;
            'lines: for (i, line) in self.lines.iter().enumerate() {
                if self.filter_matches
                    && self
                        .search_regex
                        .as_ref()
                        .is_some_and(|regex| !regex.is_match(line))
                {
                    continue;
                }
                self.max_width = self.max_width.max(line.width());
                let mut start = 0;
                let mut used = 0;
                if self.wrap {
                    for (at, grapheme) in line.grapheme_indices(true) {
                        let size = grapheme.width();
                        if used + size > self.width && at > start {
                            self.visual.push(Segment {
                                line: i,
                                bytes: start..at,
                            });
                            if self.visual.len() >= MAX_VISUAL {
                                self.layout_partial = true;
                                break 'lines;
                            }
                            start = at;
                            used = 0;
                        }
                        used += size;
                    }
                }
                self.visual.push(Segment {
                    line: i,
                    bytes: start..line.len(),
                });
                if self.visual.len() >= MAX_VISUAL {
                    self.layout_partial = true;
                    break;
                }
            }
            self.go_to(anchor);
            self.layout_key = Some(key);
        }
        if self.follow {
            self.scroll = self.visual.len().saturating_sub(self.page_size);
        }
        self.scroll = self.scroll.min(self.visual.len().saturating_sub(1));
        self.horizontal = (self.horizontal as usize).min(self.max_width.saturating_sub(1)) as u16;
    }
    pub fn visible_lines(&self) -> impl Iterator<Item = &str> {
        self.visual
            .iter()
            .skip(self.scroll)
            .take(self.page_size)
            .map(|v| &self.lines[v.line][v.bytes.clone()])
    }
    pub fn navigate(&mut self, action: Action) {
        self.follow = false;
        self.scroll = match action {
            Action::Down => self.scroll.saturating_add(1),
            Action::Up => self.scroll.saturating_sub(1),
            Action::First => 0,
            Action::Last => {
                self.follow = self.streaming;
                self.visual.len().saturating_sub(self.page_size)
            }
            Action::PageDown => self.scroll.saturating_add(self.page_size),
            Action::PageUp => self.scroll.saturating_sub(self.page_size),
            _ => self.scroll,
        }
        .min(self.visual.len().saturating_sub(1));
    }
    pub fn set_search(&mut self, text: String) {
        self.search = text;
        self.search_regex = (!self.search.is_empty())
            .then(|| {
                RegexBuilder::new(&regex::escape(&self.search))
                    .case_insensitive(true)
                    .size_limit(1_048_576)
                    .build()
                    .ok()
            })
            .flatten();
        self.index_search();
        self.layout_key = None;
        self.search_next(false);
    }
    pub fn toggle_log_filter(&mut self) {
        self.filter_matches = !self.filter_matches;
        self.layout_key = None;
    }
    fn index_search(&mut self) {
        self.search_dirty = false;
        self.hits.clear();
        self.hit = None;
        self.matches_partial = false;
        if let Some(regex) = &self.search_regex {
            'lines: for (line, text) in self.lines.iter().enumerate() {
                for m in regex.find_iter(text) {
                    if self.hits.len() == MAX_MATCHES {
                        self.matches_partial = true;
                        break 'lines;
                    }
                    self.hits.push((line, m.start()));
                }
            }
        }
    }
    pub fn search_next(&mut self, reverse: bool) {
        self.layout(self.width, self.page_size);
        if self.hits.is_empty() {
            return;
        }
        let len = self.hits.len();
        let i = self
            .hit
            .map(|i| {
                if reverse {
                    (i + len - 1) % len
                } else {
                    (i + 1) % len
                }
            })
            .unwrap_or_else(|| {
                self.hits
                    .iter()
                    .position(|hit| *hit >= self.anchor())
                    .unwrap_or(0)
            });
        self.hit = Some(i);
        self.follow = false;
        let anchor = self.hits[i];
        self.go_to(anchor);
        if !self.wrap {
            self.horizontal = self.lines[anchor.0][..anchor.1]
                .width()
                .min(u16::MAX as usize) as u16;
        }
    }
    pub fn status(&self) -> String {
        let freshness = if let Some(status) = &self.session_state {
            status.label()
        } else {
            match &self.freshness {
                Freshness::Local => {
                    if self.streaming {
                        "stream".into()
                    } else {
                        "local".into()
                    }
                }
                Freshness::Snapshot(time) => format!("snapshot {}", time.format("%H:%M:%S")),
                Freshness::Refreshing => "STALE · refreshing".into(),
                Freshness::Error(error) => format!("NOT CURRENT · {error}"),
            }
        };
        let search = if self.search.is_empty() {
            String::new()
        } else {
            format!(
                " · matches {}/{}{}",
                self.hit.map_or(0, |i| i + 1),
                self.hits.len(),
                if self.matches_partial { "+" } else { "" }
            )
        };
        let logs = if self.session.is_some() {
            format!(
                " · {}{} · evicted {}{}",
                if self.follow {
                    "following"
                } else {
                    "paused display"
                },
                if self.filter_matches {
                    " · matching lines"
                } else {
                    ""
                },
                self.evicted,
                if self.source_errors.is_empty() {
                    String::new()
                } else {
                    format!(" · PARTIAL: {} source errors", self.source_errors.len())
                }
            )
        } else {
            String::new()
        };
        format!(
            "line {}/{} · col {} · wrap {} · {}{}{}{}",
            self.anchor().0 + usize::from(!self.lines.is_empty()),
            self.lines.len(),
            self.horizontal,
            if self.wrap { "on" } else { "off" },
            freshness,
            logs,
            search,
            if self.truncated || self.layout_partial {
                " · PARTIAL: viewer limit"
            } else {
                ""
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_tail_retains_anchor_on_eviction_and_filter_search_is_batched() {
        let mut doc = Document::new("logs".into(), String::new());
        doc.wrap = false;
        for i in 0..MAX_LINES {
            doc.append(format!("line-{i}"));
        }
        doc.layout(80, 10);
        doc.scroll = 100;
        let before = doc.visible_lines().next().expect("visible").to_owned();
        doc.append("incoming".into());
        doc.layout(80, 10);
        assert_eq!(doc.visible_lines().next(), Some(before.as_str()));
        assert_eq!(doc.evicted, 1);
        doc.set_search("incoming".into());
        doc.toggle_log_filter();
        doc.append("incoming too".into());
        assert!(doc.search_dirty);
        doc.layout(80, 10);
        assert!(!doc.search_dirty);
        assert_eq!(doc.hits.len(), 2);
        assert!(doc.visible_lines().all(|line| line.contains("incoming")));
        doc.set_search("absent".into());
        doc.layout(80, 10);
        assert_eq!(doc.visible_lines().count(), 0);
        doc.replace(String::new());
        assert_eq!(doc.evicted, 0);
    }
    #[test]
    fn wrapping_scroll_resize_and_unicode_keep_logical_anchor() {
        let mut doc = Document::new(
            "test".into(),
            "1234567890abcdefghij\n界界e\u{301}👨‍👩‍👧‍👦end\nlast".into(),
        );
        doc.layout(5, 2);
        assert_eq!(
            doc.visible_lines().collect::<Vec<_>>(),
            vec!["12345", "67890"]
        );
        doc.navigate(Action::Down);
        assert_eq!(doc.visible_lines().next(), Some("67890"));
        doc.layout(10, 2);
        assert_eq!(doc.anchor(), (0, 0));
        doc.set_search("end".into());
        assert_eq!(doc.anchor().0, 1);
        doc.wrap = false;
        doc.layout(10, 2);
        assert_eq!(doc.anchor().0, 1);
        assert!(
            doc.visible_lines()
                .next()
                .expect("line")
                .contains("e\u{301}👨‍👩‍👧‍👦")
        );
    }
    #[test]
    fn search_next_previous_empty_matches_and_refresh_retention() {
        let mut doc = Document::new("test".into(), "one hit hit\nsecond\nlast hit".into());
        doc.set_search("hit".into());
        assert!(doc.status().contains("matches 1/3"));
        doc.search_next(false);
        assert!(doc.status().contains("matches 2/3"));
        doc.search_next(true);
        assert!(doc.status().contains("matches 1/3"));
        doc.navigate(Action::Down);
        doc.replace("new\none hit hit\nsecond\nlast hit".into());
        assert_eq!(doc.anchor().0, 2);
        doc.set_search("absent".into());
        assert!(doc.status().contains("matches 0/0"));
        doc.replace("changed".into());
        assert_eq!(doc.scroll, 0);
    }
    #[test]
    fn documents_and_streams_are_bounded_and_sanitized() {
        let mut doc = Document::new("\x1btest".into(), ("line\x1b[0m\n").repeat(MAX_LINES + 1));
        assert_eq!(doc.lines.len(), MAX_LINES);
        assert!(doc.truncated);
        assert_eq!(doc.title, "test");
        assert!(!doc.lines[0].contains('\x1b'));
        doc.append("last".into());
        assert_eq!(doc.lines.len(), MAX_LINES);
    }
}
