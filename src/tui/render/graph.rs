//! Commit graph lane assignment and edge drawing.
//!
//! The renderer walks the loaded history once and emits one row per commit.
//! Every cell is built from a direction mask rather than a hand-picked glyph,
//! so crossings, merges, and branch points compose instead of special-casing
//! each other. Lanes are pure presentation: nothing here escapes into domain or
//! protocol types.

use std::collections::{BTreeSet, HashMap};

use ratatui::{style::Modifier, text::Span};

use crate::domain::{Commit, Oid};

use super::theme::{GraphGlyphs, RenderContext};

const UP: u8 = 1 << 0;
const DOWN: u8 = 1 << 1;
const LEFT: u8 = 1 << 2;
const RIGHT: u8 = 1 << 3;

/// Cells per lane: the lane column plus the gap the horizontal runs cross.
const LANE_WIDTH: usize = 2;

/// One rendered graph row: the node's lane plus a cell per column.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(in crate::tui) struct GraphRow {
    cells: Vec<Cell>,
    color: usize,
    folded_lanes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Cell {
    mask: u8,
    /// A commit node or explicit fold marker, overriding the mask glyph.
    node: Option<char>,
    /// Stable branch color, independent of this cell's projected lane.
    lane: usize,
}

impl GraphRow {
    pub(super) fn width(&self) -> usize {
        self.cells.len()
    }

    /// Stable branch identity, independent of a reused or folded column.
    pub(super) fn color(&self) -> usize {
        self.color
    }

    /// Peak number of logical lanes sharing the marked final column this row.
    /// Zero means every lane has its own column.
    pub(super) fn folded_lanes(&self) -> usize {
        self.folded_lanes
    }

    /// Render the row as colored spans, one span per contiguous run of cells
    /// that share a color.
    pub(super) fn spans(&self, context: &RenderContext) -> Vec<Span<'static>> {
        self.spans_with_highlight(context, None)
    }

    pub(super) fn spans_with_highlight(
        &self,
        context: &RenderContext,
        highlight: Option<usize>,
    ) -> Vec<Span<'static>> {
        let glyphs = context.glyphs().graph;
        self.cells
            .chunk_by(|left, right| left.lane == right.lane)
            .map(|cells| {
                let text = cells
                    .iter()
                    .map(|cell| cell.node.unwrap_or_else(|| glyph(cell.mask, glyphs)))
                    .collect();
                branch_span(text, cells[0].lane, context, highlight)
            })
            .collect()
    }

    #[cfg(test)]
    pub(super) fn to_text(&self, glyphs: GraphGlyphs) -> String {
        self.cells
            .iter()
            .map(|cell| cell.node.unwrap_or_else(|| glyph(cell.mask, glyphs)))
            .collect()
    }
}

fn branch_span(
    text: String,
    color: usize,
    context: &RenderContext,
    highlight: Option<usize>,
) -> Span<'static> {
    let mut style = context.style(context.lane_color(color));
    if highlight == Some(color) {
        style = style.add_modifier(Modifier::BOLD);
    }
    Span::styled(text, style)
}

fn glyph(mask: u8, glyphs: GraphGlyphs) -> char {
    match mask {
        0 => ' ',
        m if m == UP | DOWN | LEFT | RIGHT => glyphs.cross,
        m if m == UP | DOWN | LEFT => glyphs.tee_left,
        m if m == UP | DOWN | RIGHT => glyphs.tee_right,
        m if m == UP | LEFT | RIGHT => glyphs.tee_up,
        m if m == DOWN | LEFT | RIGHT => glyphs.tee_down,
        m if m == UP | LEFT => glyphs.up_left,
        m if m == UP | RIGHT => glyphs.up_right,
        m if m == DOWN | LEFT => glyphs.down_left,
        m if m == DOWN | RIGHT => glyphs.down_right,
        m if m & (LEFT | RIGHT) != 0 => glyphs.horizontal,
        _ => glyphs.vertical,
    }
}

/// How many lanes fit beside the commit text at this width.
pub(super) fn lane_limit(width: u16) -> usize {
    match width {
        0..=49 => 3,
        50..=89 => 5,
        _ => 8,
    }
}

/// Build one graph row per commit in `commits[..end]`.
///
/// Rows must be derived from the start of the loaded history, not from the
/// visible window: a lane only exists because some earlier commit opened it.
#[cfg(test)]
pub(super) fn graph_rows(
    commits: &[Commit],
    end: usize,
    lane_limit: usize,
    glyphs: GraphGlyphs,
) -> Vec<GraphRow> {
    let mut cache = GraphCache::default();
    cache.rows(commits, end, lane_limit, glyphs);
    cache.rows
}

/// Append-only topology cache. The owner must clear it when replacing history
/// or changing repository/ref scope; navigating backward retains the prefix.
#[derive(Debug, Default)]
pub(in crate::tui) struct GraphCache {
    rows: Vec<GraphRow>,
    state: Lanes,
    projection: Option<(usize, [char; 3])>,
    #[cfg(test)]
    steps: usize,
}

impl GraphCache {
    pub(in crate::tui) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(in crate::tui) fn rows(
        &mut self,
        commits: &[Commit],
        end: usize,
        lane_limit: usize,
        glyphs: GraphGlyphs,
    ) -> &[GraphRow] {
        let lane_limit = lane_limit.max(1);
        let projection = (lane_limit, [glyphs.commit, glyphs.merge, glyphs.root]);
        if self.projection != Some(projection) || commits.len() < self.rows.len() {
            self.clear();
            self.projection = Some(projection);
        }
        let end = end.min(commits.len());
        for commit in commits.iter().take(end).skip(self.rows.len()) {
            self.rows.push(self.state.step(commit, lane_limit, glyphs));
            #[cfg(test)]
            {
                self.steps += 1;
            }
        }
        &self.rows[..end]
    }
}

#[derive(Debug, Default)]
struct Lanes {
    /// Slots retain branch colors, including lanes beyond the render width.
    slots: Vec<Option<usize>>,
    /// One parent may have several incoming branches, ordered by logical lane.
    pending: HashMap<Oid, BTreeSet<usize>>,
    /// Ordered indexes avoid scanning an unbounded slot vector each row.
    free: BTreeSet<usize>,
    active: BTreeSet<usize>,
    next_color: usize,
}

/// Only the bounded visible prefix is copied, never all logical lanes.
struct LaneView {
    colors: Vec<Option<usize>>,
    extent: usize,
    occupied: usize,
    node_pending: bool,
}

impl LaneView {
    fn folded_count(&self, column: usize, node: usize) -> usize {
        let lanes = self.occupied - self.colors.iter().take(column).flatten().count();
        if node >= column && !self.node_pending {
            return lanes + 1;
        }
        lanes
    }
}

impl Lanes {
    fn reserve(&mut self, preferred: usize) -> (usize, usize) {
        let free = self
            .free
            .range(preferred..)
            .next()
            .copied()
            .or_else(|| self.free.first().copied());
        let lane = match free {
            Some(lane) => {
                self.free.remove(&lane);
                lane
            }
            None => {
                self.slots.push(None);
                self.slots.len() - 1
            }
        };
        let color = self.next_color;
        self.next_color += 1;
        (lane, color)
    }

    fn hold(&mut self, lane: usize, color: usize, parent: &Oid) {
        self.slots[lane] = Some(color);
        self.active.insert(lane);
        self.free.remove(&lane);
        self.pending.entry(parent.clone()).or_default().insert(lane);
    }

    fn release(&mut self, lane: usize) {
        self.slots[lane] = None;
        self.active.remove(&lane);
        self.free.insert(lane);
    }

    fn view(&self, limit: usize, node: usize) -> LaneView {
        LaneView {
            colors: self.slots.iter().take(limit).copied().collect(),
            extent: self.active.last().map_or(0, |lane| lane + 1),
            occupied: self.active.len(),
            node_pending: self.slots[node].is_some(),
        }
    }

    fn parent_branch(&mut self, parent: &Oid, preferred: usize) -> (usize, usize) {
        if let Some(lane) = self.pending.get(parent).and_then(BTreeSet::first).copied() {
            return (lane, self.slots[lane].expect("pending lane has a color"));
        }
        let (lane, color) = self.reserve(preferred);
        self.hold(lane, color, parent);
        (lane, color)
    }

    fn step(&mut self, commit: &Commit, limit: usize, glyphs: GraphGlyphs) -> GraphRow {
        let incoming: Vec<_> = self
            .pending
            .remove(&commit.id)
            .unwrap_or_default()
            .into_iter()
            .map(|lane| (lane, self.slots[lane].expect("pending lane has a color")))
            .collect();
        let (node, color) = incoming.first().copied().unwrap_or_else(|| self.reserve(0));
        let before = self.view(limit, node);
        for &(lane, _) in &incoming {
            self.release(lane);
        }
        // A first parent inherits the branch identity even when another lane
        // also waits for it; those lanes join only at the actual parent row.
        if let Some(parent) = commit.parents.first() {
            self.hold(node, color, parent);
        } else {
            self.release(node);
        }
        let branches: Vec<_> = commit
            .parents
            .iter()
            .skip(1)
            .map(|parent| self.parent_branch(parent, node + 1))
            .collect();
        let after = self.view(limit, node);
        let mut row = project(&before, &after, node, color, limit);
        for &(lane, color) in incoming.iter().skip(1) {
            run(&mut row.cells, node, lane, UP, color);
        }
        for (lane, color) in branches {
            run(&mut row.cells, node, lane, DOWN, color);
        }
        row.place_node(node, node_glyph(commit, glyphs));
        row
    }
}

fn node_glyph(commit: &Commit, glyphs: GraphGlyphs) -> char {
    match commit.parents.len() {
        0 => glyphs.root,
        1 => glyphs.commit,
        _ => glyphs.merge,
    }
}

fn project(
    before: &LaneView,
    after: &LaneView,
    node: usize,
    color: usize,
    limit: usize,
) -> GraphRow {
    let extent = before.extent.max(after.extent).max(node + 1);
    let visible = extent.min(limit);
    let mut cells = vec![Cell::default(); visible * LANE_WIDTH];
    for lane in 0..visible {
        let cell = &mut cells[lane * LANE_WIDTH];
        if let Some(color) = before.colors.get(lane).copied().flatten() {
            cell.mask |= UP;
            cell.lane = color;
        }
        if let Some(color) = after.colors.get(lane).copied().flatten() {
            cell.mask |= DOWN;
            cell.lane = color;
        }
    }
    let folded_lanes = if extent > limit {
        before
            .folded_count(limit - 1, node)
            .max(after.folded_count(limit - 1, node))
    } else {
        0
    };
    GraphRow {
        cells,
        color,
        folded_lanes,
    }
}

impl GraphRow {
    fn place_node(&mut self, node: usize, glyph: char) {
        let last = self.cells.len() / LANE_WIDTH - 1;
        let column = node.min(last) * LANE_WIDTH;
        self.cells[column].node = Some(glyph);
        self.cells[column].lane = self.color;
        if self.folded_lanes > 0 {
            // A folded column is an explicit bundle, never an apparent exact
            // edge. Keep a hidden commit's node, marking its adjacent gap too.
            let folded = &mut self.cells[last * LANE_WIDTH];
            if node < last {
                folded.node = Some('~');
            }
            self.cells[last * LANE_WIDTH + 1].node = Some('~');
        }
    }
}

/// Draw the horizontal run that ties `target` back to the node's lane, adding
/// `terminal` (up for a lane closing into the node, down for a new parent lane)
/// at the far end.
fn run(cells: &mut [Cell], node_lane: usize, target: usize, terminal: u8, color_lane: usize) {
    let visible = cells.len() / LANE_WIDTH;
    let node = node_lane.min(visible.saturating_sub(1));
    let target = target.min(visible.saturating_sub(1));
    if node == target {
        cells[node * LANE_WIDTH].mask |= terminal;
        return;
    }
    let (low, high) = (node.min(target), node.max(target));
    let (from_bit, to_bit) = if target > node {
        (RIGHT, LEFT)
    } else {
        (LEFT, RIGHT)
    };
    cells[node * LANE_WIDTH].mask |= from_bit;
    cells[target * LANE_WIDTH].mask |= to_bit | terminal;
    for cell in cells
        .iter_mut()
        .take(high * LANE_WIDTH)
        .skip(low * LANE_WIDTH + 1)
    {
        cell.mask |= LEFT | RIGHT;
        if cell.mask & (UP | DOWN) == 0 {
            cell.lane = color_lane;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ObjectFormat, Signature};

    fn oid(seed: u8) -> Oid {
        Oid::parse_with_format(&format!("{seed:02x}").repeat(20), ObjectFormat::Sha1)
            .expect("valid oid")
    }

    fn commit(id: u8, parents: &[u8]) -> Commit {
        let signature = Signature {
            name: "Test".into(),
            email: "test@example.com".into(),
            timestamp: 0,
            timezone: "+0000".into(),
        };
        Commit {
            id: oid(id),
            parents: parents.iter().copied().map(oid).collect(),
            author: signature.clone(),
            committer: signature,
            decorations: Vec::new(),
            subject: format!("commit {id}"),
            body: String::new(),
        }
    }

    fn render(commits: &[Commit], lane_limit: usize) -> Vec<String> {
        let glyphs = super::super::theme::RenderContext::new(Default::default())
            .glyphs()
            .graph;
        graph_rows(commits, commits.len(), lane_limit, glyphs)
            .iter()
            .map(|row| row.to_text(glyphs))
            .collect()
    }

    #[test]
    fn linear_history_draws_one_lane() {
        let commits = [commit(1, &[2]), commit(2, &[3]), commit(3, &[])];
        assert_eq!(render(&commits, 8), ["● ", "● ", "◌ "]);
    }

    #[test]
    fn merge_opens_a_lane_and_the_join_closes_it() {
        // 1 merges 2 and 3; both parents reach 4, so lane one closes at 3.
        let commits = [
            commit(1, &[2, 3]),
            commit(2, &[4]),
            commit(3, &[4]),
            commit(4, &[]),
        ];
        assert_eq!(render(&commits, 8), ["◆─╮ ", "● │ ", "│ ● ", "◌─╯ "]);
    }

    #[test]
    fn a_join_closes_the_far_lane() {
        // 1 and 2 are independent tips whose histories meet at 5.
        let commits = [
            commit(1, &[3]),
            commit(2, &[4]),
            commit(3, &[5]),
            commit(4, &[5]),
            commit(5, &[]),
        ];
        let rows = render(&commits, 8);
        assert_eq!(rows[0], "● ");
        assert_eq!(rows[1], "│ ● ");
        assert_eq!(rows[4], "◌─╯ ");
    }

    #[test]
    fn a_run_crossing_a_live_lane_draws_a_crossing() {
        // Lanes zero and two both wait for 4 while lane one still waits for 5,
        // so the join at 4 has to cross a live lane.
        let commits = [
            commit(1, &[4]),
            commit(2, &[5]),
            commit(3, &[4]),
            commit(4, &[]),
        ];
        let rows = render(&commits, 8);
        assert_eq!(rows[2], "│ │ ● ");
        assert_eq!(rows[3], "◌─┼─╯ ");
    }

    #[test]
    fn root_commits_use_the_root_glyph() {
        assert_eq!(render(&[commit(1, &[])], 8), ["◌ "]);
    }

    #[test]
    fn lane_limit_folds_instead_of_growing_without_bound() {
        // Five independent tips with only two lanes available.
        let commits: Vec<Commit> = (1..=5).map(|id| commit(id, &[id + 10])).collect();
        for row in render(&commits, 2) {
            assert!(row.chars().count() <= 2 * LANE_WIDTH, "row too wide: {row}");
        }
    }

    fn graph_glyphs() -> GraphGlyphs {
        RenderContext::new(Default::default()).glyphs().graph
    }

    #[test]
    fn overflow_preserves_every_pending_parent_and_branch_identity() {
        let mut commits: Vec<_> = (1..=5).map(|id| commit(id, &[id + 10])).collect();
        let mut cache = GraphCache::default();
        cache.rows(&commits, commits.len(), 2, graph_glyphs());
        assert_eq!(cache.state.pending.len(), 5);
        assert_eq!(cache.state.active.len(), 5);
        assert_eq!(cache.rows[4].folded_lanes(), 4);
        assert_eq!(cache.rows[4].to_text(graph_glyphs()), "│ ●~");
        for id in 1..=5 {
            assert_eq!(
                cache.state.pending[&oid(id + 10)],
                BTreeSet::from([usize::from(id - 1)])
            );
        }
        commits.extend((11..=15).map(|id| commit(id, &[])));
        let rows = cache.rows(&commits, commits.len(), 2, graph_glyphs());
        for tip in 0..5 {
            assert_eq!(rows[tip].color(), rows[tip + 5].color());
        }
        assert_eq!(rows[5].to_text(graph_glyphs()), "◌ ~~");
        assert!(cache.state.pending.is_empty());
        assert!(cache.state.active.is_empty());
    }

    #[test]
    fn octopus_shared_parents_and_unrelated_tips_survive_folding() {
        let commits = [
            commit(1, &[4, 5, 6, 6]),
            commit(2, &[5, 7]),
            commit(3, &[8]),
            commit(4, &[]),
            commit(5, &[]),
            commit(6, &[]),
            commit(7, &[]),
            commit(8, &[]),
        ];
        let mut cache = GraphCache::default();
        cache.rows(&commits, 3, 2, graph_glyphs());
        assert_eq!(cache.state.active.len(), 6);
        assert_eq!(cache.state.pending[&oid(5)].len(), 2);
        assert_eq!(cache.state.pending[&oid(6)].len(), 1);
        assert_eq!(cache.rows[2].folded_lanes(), 5);
        cache.rows(&commits, commits.len(), 2, graph_glyphs());
        assert!(cache.state.pending.is_empty());
        assert!(cache.state.active.is_empty());
        assert!(cache.rows.iter().all(|row| row.width() <= 4));
    }

    #[test]
    fn connectors_follow_their_branch_color_and_preserve_crossings() {
        let closing = [
            commit(1, &[4]),
            commit(2, &[5]),
            commit(3, &[4]),
            commit(4, &[]),
        ];
        let rows = graph_rows(&closing, closing.len(), 8, graph_glyphs());
        assert_eq!(rows[3].cells[1].lane, rows[2].color());
        assert_eq!(rows[3].cells[3].lane, rows[2].color());
        assert_eq!(rows[3].cells[2].lane, rows[1].color());
        assert_eq!(rows[3].cells[4].lane, rows[2].color());

        let opening = [commit(1, &[6]), commit(2, &[7]), commit(3, &[8, 6])];
        let rows = graph_rows(&opening, opening.len(), 8, graph_glyphs());
        assert_eq!(rows[2].to_text(graph_glyphs()), "├─┼─◆ ");
        assert_eq!(rows[2].cells[1].lane, rows[0].color());
        assert_eq!(rows[2].cells[3].lane, rows[0].color());
        assert_eq!(rows[2].cells[2].lane, rows[1].color());
    }

    #[test]
    fn new_branch_in_a_reused_slot_gets_its_own_color() {
        let commits = [commit(1, &[]), commit(2, &[3]), commit(3, &[])];
        let rows = graph_rows(&commits, commits.len(), 8, graph_glyphs());
        assert_ne!(rows[0].color(), rows[1].color());
        assert_eq!(rows[1].color(), rows[2].color());
    }

    #[test]
    fn unrelated_root_counts_as_a_folded_lane_only_on_its_own_row() {
        let commits = [
            commit(1, &[4]),
            commit(2, &[5]),
            commit(3, &[]),
            commit(4, &[]),
            commit(5, &[]),
        ];
        let rows = graph_rows(&commits, commits.len(), 2, graph_glyphs());
        assert_eq!(rows[2].folded_lanes(), 2);
        assert_eq!(rows[2].to_text(graph_glyphs()), "│ ◌~");
        assert_eq!(rows[3].folded_lanes(), 0);
        assert_eq!(rows[3].to_text(graph_glyphs()), "◌ │ ");
    }

    #[test]
    fn incremental_cache_matches_fresh_rows_without_repeating_steps() {
        let commits = [
            commit(1, &[4, 5, 6]),
            commit(2, &[5, 7]),
            commit(3, &[8]),
            commit(4, &[9]),
            commit(5, &[9]),
            commit(6, &[9]),
            commit(7, &[]),
            commit(8, &[]),
            commit(9, &[]),
        ];
        for limit in [0, 1, 2, 8] {
            let mut cache = GraphCache::default();
            for end in 0..=commits.len() {
                let expected = graph_rows(&commits, end, limit, graph_glyphs());
                assert_eq!(
                    cache.rows(&commits[..end], end, limit, graph_glyphs()),
                    expected
                );
                assert_eq!(cache.steps, end);
            }
            for end in [3, 0, 7, 1, commits.len()] {
                assert_eq!(
                    cache.rows(&commits, end, limit, graph_glyphs()),
                    graph_rows(&commits, end, limit, graph_glyphs())
                );
                assert_eq!(cache.steps, commits.len());
            }
        }
    }

    #[test]
    fn cache_reprojects_width_and_glyph_changes_and_clears_replaced_history() {
        use super::super::theme::{GlyphMode, RenderConfig};

        let commits = [commit(1, &[4, 5, 6]), commit(2, &[7]), commit(3, &[8])];
        let ascii = RenderContext::new(RenderConfig {
            glyph_mode: GlyphMode::Ascii,
            ..Default::default()
        })
        .glyphs()
        .graph;
        let mut cache = GraphCache::default();
        cache.rows(&commits, commits.len(), 8, graph_glyphs());
        for limit in [2, 1, 0, 8] {
            let rows = cache.rows(&commits, usize::MAX, limit, ascii);
            assert_eq!(rows, graph_rows(&commits, commits.len(), limit, ascii));
            assert!(rows.iter().all(|row| row.to_text(ascii).is_ascii()));
            assert!(
                rows.iter()
                    .all(|row| row.width() <= limit.max(1) * LANE_WIDTH)
            );
        }
        cache.clear();
        let replacement = [commit(30, &[31]), commit(31, &[]), commit(32, &[])];
        assert_eq!(
            cache.rows(&replacement, 3, 8, ascii),
            graph_rows(&replacement, 3, 8, ascii)
        );
        assert_eq!(cache.steps, 3);
    }

    #[test]
    fn many_independent_tips_keep_indexed_pending_lanes() {
        let commits: Vec<_> = (1..=10_000)
            .map(|id| {
                let mut tip = commit(1, &[]);
                tip.id = Oid::parse_with_format(&format!("{id:040x}"), ObjectFormat::Sha1).unwrap();
                tip.parents = vec![
                    Oid::parse_with_format(&format!("{:040x}", id + 10_000), ObjectFormat::Sha1)
                        .unwrap(),
                ];
                tip
            })
            .collect();
        let mut cache = GraphCache::default();
        cache.rows(&commits, commits.len(), 3, graph_glyphs());
        assert_eq!(cache.state.pending.len(), 10_000);
        assert_eq!(cache.state.active.len(), 10_000);
        assert_eq!(cache.rows.last().unwrap().folded_lanes(), 9_998);
        cache.rows(&commits, 10, 3, graph_glyphs());
        assert_eq!(cache.steps, 10_000);
    }

    #[test]
    fn selected_branch_spans_are_bold_without_recoloring_other_lanes() {
        let context = RenderContext::new(Default::default());
        let commits = [commit(1, &[3]), commit(2, &[4])];
        let rows = graph_rows(&commits, 2, 8, graph_glyphs());
        let plain = rows[1].spans(&context);
        let highlighted = rows[1].spans_with_highlight(&context, Some(rows[1].color()));
        assert_eq!(plain.len(), highlighted.len());
        for (plain, highlighted) in plain.iter().zip(&highlighted) {
            assert_eq!(plain.content, highlighted.content);
            assert_eq!(plain.style.fg, highlighted.style.fg);
        }
        assert!(
            highlighted
                .iter()
                .any(|span| span.style.add_modifier.contains(Modifier::BOLD))
        );
        assert!(!highlighted[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn every_mask_resolves_to_a_glyph() {
        let glyphs = super::super::theme::RenderContext::new(Default::default())
            .glyphs()
            .graph;
        for mask in 0..16u8 {
            let rendered = glyph(mask, glyphs);
            assert!(
                mask == 0 || rendered != ' ',
                "mask {mask:04b} rendered as a blank"
            );
        }
    }
}
