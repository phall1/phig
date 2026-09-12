//! Named refs on the commit graph.
//!
//! Git still owns the decoration strings. This module classifies them for
//! display so HEAD, local branches, remotes, and tags stay distinct without
//! relying on color alone.

use ratatui::{style::Style, text::Span};

use super::{
    format::{display_width, truncate_with},
    theme::RenderContext,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Decoration {
    Head,
    HeadBranch(String),
    Local(String),
    Remote(String),
    Tag(String),
}

impl Decoration {
    fn display(&self, arrow: &str) -> String {
        match self {
            Self::Head => "HEAD".into(),
            Self::HeadBranch(branch) => format!("HEAD{arrow}{branch}"),
            Self::Local(name) | Self::Remote(name) => name.clone(),
            Self::Tag(name) => format!("tag:{name}"),
        }
    }
}

pub(super) fn parse_decorations(raw: &[String]) -> Vec<Decoration> {
    raw.iter().filter_map(|item| parse_one(item)).collect()
}

fn parse_one(raw: &str) -> Option<Decoration> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw == "HEAD" {
        return Some(Decoration::Head);
    }
    if let Some(branch) = raw.strip_prefix("HEAD -> ") {
        let branch = branch.trim();
        if branch.is_empty() {
            return Some(Decoration::Head);
        }
        return Some(Decoration::HeadBranch(branch.to_owned()));
    }
    if let Some(tag) = raw.strip_prefix("tag: ") {
        let tag = tag.trim();
        if tag.is_empty() {
            return None;
        }
        return Some(Decoration::Tag(tag.to_owned()));
    }
    if raw.contains('/') {
        return Some(Decoration::Remote(raw.to_owned()));
    }
    Some(Decoration::Local(raw.to_owned()))
}

/// Stable name for a graph lane, taken from the first decorated commit that
/// opened it. HEAD itself is a pointer, not a branch, so it is skipped.
pub(super) fn lane_name(raw: &[String]) -> Option<String> {
    let mut remote = None;
    let mut tag = None;
    for decoration in parse_decorations(raw) {
        match decoration {
            Decoration::HeadBranch(name) | Decoration::Local(name) => return Some(name),
            Decoration::Remote(name) if remote.is_none() => remote = Some(name),
            Decoration::Tag(name) if tag.is_none() => tag = Some(format!("tag:{name}")),
            _ => {}
        }
    }
    remote.or(tag)
}

/// Fit named refs into `width` cells. The caller adds a trailing space.
///
/// `lane` colors local branch names to match the graph. Inherited names, used
/// when a tip has scrolled away, stay muted so they cannot be mistaken for a
/// ref that points at this commit.
pub(super) fn decoration_spans(
    raw: &[String],
    inherited: Option<&str>,
    width: usize,
    lane: Option<usize>,
    context: &RenderContext,
) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let owned;
    let decorations = if let Some(label) = inherited {
        owned = vec![inherited_decoration(label)];
        &owned
    } else {
        owned = parse_decorations(raw);
        &owned
    };
    if decorations.is_empty() {
        return Vec::new();
    }
    fit_badges(decorations, width, lane, inherited.is_some(), context)
}

fn inherited_decoration(label: &str) -> Decoration {
    parse_one(label).unwrap_or_else(|| Decoration::Local(label.to_owned()))
}

fn fit_badges(
    decorations: &[Decoration],
    width: usize,
    lane: Option<usize>,
    inherited: bool,
    context: &RenderContext,
) -> Vec<Span<'static>> {
    let arrow = context.glyphs().arrow;
    let ellipsis = context.glyphs().ellipsis;
    let mut spans = Vec::new();
    let mut remaining = width;
    for (index, decoration) in decorations.iter().enumerate() {
        let separator = usize::from(index > 0);
        if remaining <= separator {
            break;
        }
        let available = remaining - separator;
        let label = decoration.display(arrow);
        let label_width = display_width(&label);
        if label_width <= available {
            if separator == 1 {
                spans.push(Span::raw(" "));
            }
            spans.extend(decoration_badge(decoration, lane, inherited, context));
            remaining -= separator + label_width;
            continue;
        }
        if index == 0 {
            let truncated = truncate_with(&label, available, ellipsis);
            if !truncated.is_empty() {
                spans.push(Span::styled(
                    truncated,
                    decoration_style(decoration, lane, inherited, context),
                ));
            }
        }
        break;
    }
    spans
}

fn decoration_badge(
    decoration: &Decoration,
    lane: Option<usize>,
    inherited: bool,
    context: &RenderContext,
) -> Vec<Span<'static>> {
    if inherited {
        return vec![Span::styled(
            decoration.display(context.glyphs().arrow),
            context.style(context.muted()),
        )];
    }
    match decoration {
        Decoration::Head => vec![Span::styled(
            "HEAD".to_owned(),
            context.strong(context.accent()),
        )],
        Decoration::HeadBranch(branch) => vec![
            Span::styled("HEAD".to_owned(), context.strong(context.accent())),
            Span::styled(
                context.glyphs().arrow.to_owned(),
                context.style(context.muted()),
            ),
            Span::styled(branch.clone(), local_style(lane, context)),
        ],
        Decoration::Local(name) => {
            vec![Span::styled(name.clone(), local_style(lane, context))]
        }
        Decoration::Remote(name) => {
            vec![Span::styled(name.clone(), context.style(context.muted()))]
        }
        Decoration::Tag(name) => vec![Span::styled(
            format!("tag:{name}"),
            context.style(context.warning()),
        )],
    }
}

fn decoration_style(
    decoration: &Decoration,
    lane: Option<usize>,
    inherited: bool,
    context: &RenderContext,
) -> Style {
    if inherited {
        return context.style(context.muted());
    }
    match decoration {
        Decoration::Head | Decoration::HeadBranch(_) => context.strong(context.accent()),
        Decoration::Local(_) => local_style(lane, context),
        Decoration::Remote(_) => context.style(context.muted()),
        Decoration::Tag(_) => context.style(context.warning()),
    }
}

fn local_style(lane: Option<usize>, context: &RenderContext) -> Style {
    match lane {
        Some(color) => context.strong(context.lane_color(color)),
        None => context.strong(context.added()),
    }
}

pub(super) fn spans_width(spans: &[Span<'_>]) -> usize {
    spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::theme::{GlyphMode, RenderConfig, RenderContext};

    fn unicode() -> RenderContext {
        RenderContext::new(RenderConfig {
            glyph_mode: GlyphMode::Unicode,
            color_mode: crate::tui::render::theme::ColorMode::Always,
            ..RenderConfig::default()
        })
    }

    fn ascii() -> RenderContext {
        RenderContext::new(RenderConfig {
            glyph_mode: GlyphMode::Ascii,
            color_mode: crate::tui::render::theme::ColorMode::Always,
            ..RenderConfig::default()
        })
    }

    fn text(spans: &[Span<'_>]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    #[test]
    fn classifies_git_short_decorations() {
        let parsed = parse_decorations(&[
            "HEAD -> main".into(),
            "origin/main".into(),
            "tag: v1".into(),
            "topic".into(),
            "comma,name".into(),
            "HEAD".into(),
        ]);
        assert_eq!(
            parsed,
            vec![
                Decoration::HeadBranch("main".into()),
                Decoration::Remote("origin/main".into()),
                Decoration::Tag("v1".into()),
                Decoration::Local("topic".into()),
                Decoration::Local("comma,name".into()),
                Decoration::Head,
            ]
        );
    }

    #[test]
    fn lane_name_prefers_the_local_branch() {
        assert_eq!(
            lane_name(&[
                "HEAD -> main".into(),
                "origin/main".into(),
                "tag: v1".into()
            ]),
            Some("main".into())
        );
        assert_eq!(
            lane_name(&["origin/feature".into()]),
            Some("origin/feature".into())
        );
        assert_eq!(lane_name(&["tag: v1".into()]), Some("tag:v1".into()));
        assert_eq!(lane_name(&["HEAD".into()]), None);
        assert_eq!(lane_name(&[]), None);
    }

    #[test]
    fn badges_use_kind_prefixes_and_keep_comma_names_intact() {
        let context = unicode();
        let spans = decoration_spans(
            &[
                "HEAD -> main".into(),
                "comma,name".into(),
                "origin/main".into(),
                "tag: v1".into(),
            ],
            None,
            80,
            Some(0),
            &context,
        );
        assert_eq!(text(&spans), "HEAD→main comma,name origin/main tag:v1");
    }

    #[test]
    fn ascii_uses_the_plain_arrow() {
        let context = ascii();
        let spans = decoration_spans(&["HEAD -> main".into()], None, 20, Some(0), &context);
        assert_eq!(text(&spans), "HEAD->main");
    }

    #[test]
    fn drops_trailing_refs_before_truncating_head() {
        let context = unicode();
        let spans = decoration_spans(
            &[
                "HEAD -> main".into(),
                "origin/main".into(),
                "tag: v1".into(),
            ],
            None,
            9,
            Some(0),
            &context,
        );
        assert_eq!(text(&spans), "HEAD→main");
    }

    #[test]
    fn truncates_the_first_badge_when_nothing_else_fits() {
        let context = unicode();
        let spans = decoration_spans(&["HEAD -> main".into()], None, 6, Some(0), &context);
        assert_eq!(text(&spans), "HEAD→…");
    }

    #[test]
    fn inherited_names_are_muted_single_badges() {
        let context = unicode();
        let spans = decoration_spans(&[], Some("main"), 20, Some(0), &context);
        assert_eq!(text(&spans), "main");
        assert_eq!(spans[0].style.fg, Some(context.muted()));
    }
}
