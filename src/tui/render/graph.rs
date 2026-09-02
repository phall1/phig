//! Commit graph lane assignment and edge drawing.
//!
//! The renderer walks the loaded history once and emits one row per commit.
//! Every cell is built from a direction mask rather than a hand-picked glyph,
//! so crossings, merges, and branch points compose instead of special-casing
//! each other. Lanes are pure presentation: nothing here escapes into domain or
//! protocol types.

use ratatui::text::Span;

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
pub(super) struct GraphRow {
    cells: Vec<Cell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Cell {
    mask: u8,
    /// Set when a commit node occupies this cell, overriding the mask glyph.
    node: Option<char>,
    /// Lane whose color this cell adopts.
    lane: usize,
}

impl GraphRow {
    pub(super) fn width(&self) -> usize {
        self.cells.len()
    }

    /// Render the row as colored spans, one span per contiguous run of cells
    /// that share a color.
    pub(super) fn spans(&self, context: &RenderContext) -> Vec<Span<'static>> {
        let glyphs = context.glyphs().graph;
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut text = String::new();
        let mut current: Option<usize> = None;
        for cell in &self.cells {
            if current != Some(cell.lane) && !text.is_empty() {
                let lane = current.unwrap_or(0);
                spans.push(Span::styled(
                    std::mem::take(&mut text),
                    context.style(context.lane_color(lane)),
                ));
            }
            current = Some(cell.lane);
            text.push(cell.node.unwrap_or_else(|| glyph(cell.mask, glyphs)));
        }
        if !text.is_empty() {
            spans.push(Span::styled(
                text,
                context.style(context.lane_color(current.unwrap_or(0))),
            ));
        }
        spans
    }

    #[cfg(test)]
    pub(super) fn to_text(&self, glyphs: GraphGlyphs) -> String {
        self.cells
            .iter()
            .map(|cell| cell.node.unwrap_or_else(|| glyph(cell.mask, glyphs)))
            .collect()
    }
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
pub(super) fn graph_rows(
    commits: &[Commit],
    end: usize,
    lane_limit: usize,
    glyphs: GraphGlyphs,
) -> Vec<GraphRow> {
    let lane_limit = lane_limit.max(1);
    let mut lanes: Vec<Option<Oid>> = Vec::new();
    let mut rows = Vec::with_capacity(end.min(commits.len()));
    for commit in commits.iter().take(end) {
        rows.push(step(&mut lanes, commit, lane_limit, glyphs));
    }
    rows
}

fn step(
    lanes: &mut Vec<Option<Oid>>,
    commit: &Commit,
    lane_limit: usize,
    glyphs: GraphGlyphs,
) -> GraphRow {
    let incoming: Vec<usize> = lanes
        .iter()
        .enumerate()
        .filter(|(_, held)| held.as_ref() == Some(&commit.id))
        .map(|(index, _)| index)
        .collect();
    let node_lane = match incoming.first() {
        Some(lane) => *lane,
        // A commit nothing is waiting for starts its own lane: a branch tip, or
        // a root of a disjoint history brought in by a wider ref scope.
        None => reserve(lanes, 0, lane_limit),
    };
    let occupied_before: Vec<bool> = lanes.iter().map(Option::is_some).collect();

    // Lanes merging into this commit close here; the first parent inherits the
    // node's own lane and every extra parent needs a lane of its own.
    for lane in incoming.iter().skip(1) {
        lanes[*lane] = None;
    }
    lanes[node_lane] = commit.parents.first().cloned();
    let mut branches = Vec::new();
    for parent in commit.parents.iter().skip(1) {
        let lane = match lanes.iter().position(|held| held.as_ref() == Some(parent)) {
            Some(existing) => existing,
            None => {
                let lane = reserve(lanes, node_lane + 1, lane_limit);
                lanes[lane] = Some(parent.clone());
                lane
            }
        };
        branches.push(lane);
    }
    while lanes.last().is_some_and(Option::is_none) {
        lanes.pop();
    }

    let visible = occupied_before
        .len()
        .max(lanes.len())
        .max(node_lane + 1)
        .min(lane_limit);
    let mut cells = vec![Cell::default(); visible * LANE_WIDTH];
    for (lane, cell) in cells.iter_mut().enumerate() {
        cell.lane = lane / LANE_WIDTH;
    }

    // Vertical continuity: a lane connects upward when it was occupied before
    // this row and downward when it is still occupied after it.
    for lane in 0..visible {
        let column = lane * LANE_WIDTH;
        if occupied_before.get(lane).copied().unwrap_or(false) {
            cells[column].mask |= UP;
        }
        if lanes.get(lane).is_some_and(Option::is_some) {
            cells[column].mask |= DOWN;
        }
    }

    let node_column = node_lane.min(visible.saturating_sub(1)) * LANE_WIDTH;
    for lane in incoming.iter().skip(1) {
        run(&mut cells, node_lane, *lane, visible, UP, node_lane);
    }
    for lane in &branches {
        run(&mut cells, node_lane, *lane, visible, DOWN, node_lane);
    }
    cells[node_column].node = Some(if commit.parents.len() > 1 {
        glyphs.merge
    } else if commit.parents.is_empty() {
        glyphs.root
    } else {
        glyphs.commit
    });
    cells[node_column].lane = node_lane;
    GraphRow { cells }
}

/// Draw the horizontal run that ties `target` back to the node's lane, adding
/// `terminal` (up for a lane closing into the node, down for a new parent lane)
/// at the far end.
fn run(
    cells: &mut [Cell],
    node_lane: usize,
    target: usize,
    visible: usize,
    terminal: u8,
    color_lane: usize,
) {
    if target == node_lane {
        cells[node_lane.min(visible.saturating_sub(1)) * LANE_WIDTH].mask |= terminal;
        return;
    }
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

/// Take the first free lane at or after `preferred`, appending when the row is
/// full but still under the limit.
fn reserve(lanes: &mut Vec<Option<Oid>>, preferred: usize, lane_limit: usize) -> usize {
    if let Some(lane) = lanes
        .iter()
        .skip(preferred)
        .position(Option::is_none)
        .map(|offset| offset + preferred)
    {
        return lane;
    }
    if let Some(lane) = lanes.iter().position(Option::is_none) {
        return lane;
    }
    if lanes.len() < lane_limit {
        lanes.push(None);
        return lanes.len() - 1;
    }
    // Beyond the limit the graph folds into its last lane rather than pushing
    // the commit text off screen.
    lane_limit - 1
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
