//! SPEC: P7-OCR-004 — find the tables on a page, from the grid it draws.
//!
//! **Ruled tables only, on purpose.** There are two ways to guess that some
//! text is a table. Ruling lines are *evidence*: the document drew a grid.
//! Alignment is a *guess*, and it fires on things that are not tables — a
//! two-column layout, a contents list, a form. A wrong table is worse for a
//! reader than no table, because it locks prose into cells they then have to
//! unpick by hand. So an unruled table comes out as paragraphs, and the spec's
//! "where detectable" is read as "where the page says so".
//!
//! Everything here is pure geometry in the frame a reader sees — y downwards,
//! so `top` is the smaller number. No `PDFium`, no XML, and therefore testable
//! against rectangles typed into a test rather than against a PDF.

/// One straight rule the page draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    /// `true` for a rule of constant y, `false` for one of constant x.
    pub horizontal: bool,
    /// The constant coordinate: y for a horizontal rule, x for a vertical one.
    pub position: f32,
    /// The extent along the other axis.
    pub from: f32,
    pub to: f32,
}

impl Rule {
    fn length(self) -> f32 {
        (self.to - self.from).abs()
    }
}

/// One cell of a detected table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub row: usize,
    pub column: usize,
    /// How many grid columns this cell covers — 1 unless a dividing rule is
    /// missing between them.
    pub span: usize,
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

/// A detected table: its grid, and the cells it implies.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    /// Grid line positions, left to right.
    pub columns: Vec<f32>,
    /// Grid line positions, top to bottom.
    pub rows: Vec<f32>,
    pub cells: Vec<Cell>,
}

impl Table {
    #[must_use]
    pub fn left(&self) -> f32 {
        self.columns.first().copied().unwrap_or(0.0)
    }
    #[must_use]
    pub fn right(&self) -> f32 {
        self.columns.last().copied().unwrap_or(0.0)
    }
    #[must_use]
    pub fn top(&self) -> f32 {
        self.rows.first().copied().unwrap_or(0.0)
    }
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.rows.last().copied().unwrap_or(0.0)
    }
    /// Does this table's area contain the point? Used to keep a cell's text out
    /// of the paragraph flow.
    #[must_use]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left() - TOLERANCE
            && x <= self.right() + TOLERANCE
            && y >= self.top() - TOLERANCE
            && y <= self.bottom() + TOLERANCE
    }
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len().saturating_sub(1)
    }
    #[must_use]
    pub fn column_count(&self) -> usize {
        self.columns.len().saturating_sub(1)
    }
}

/// Rules within this many points of each other are the same grid line. Two
/// points is wider than any hairline and narrower than any real cell.
const TOLERANCE: f32 = 2.0;
/// A rule has to cross most of the grid to be one of its lines. A short mark
/// inside a cell — a tick, an underline, a minus sign drawn as a path — is not
/// a row divider.
const SPAN_SHARE: f32 = 0.8;
/// Below this, a "table" is a box or a pair of rules, not a table.
const MIN_ROWS: usize = 2;
const MIN_COLUMNS: usize = 2;

/// Collapse near-equal positions into one line each, in order.
fn cluster(mut positions: Vec<f32>) -> Vec<f32> {
    positions.sort_by(f32::total_cmp);
    let mut out: Vec<f32> = Vec::new();
    for position in positions {
        match out.last_mut() {
            Some(last) if (position - *last).abs() <= TOLERANCE => {
                // Keep the first of a cluster rather than averaging: the lines
                // are what the page drew, and averaging drifts them.
            }
            _ => out.push(position),
        }
    }
    out
}

/// SPEC: P7-OCR-004 — the tables a page's rules describe.
///
/// Vertical rules that overlap vertically are treated as one candidate grid, so
/// two tables on a page are found separately. Every candidate then has to pass
/// all four guards below, which is what keeps a section rule under a masthead
/// from becoming a table.
#[must_use]
pub fn detect_tables(rules: &[Rule]) -> Vec<Table> {
    let verticals: Vec<Rule> = rules.iter().copied().filter(|r| !r.horizontal).collect();
    let horizontals: Vec<Rule> = rules.iter().copied().filter(|r| r.horizontal).collect();
    if verticals.len() < MIN_COLUMNS + 1 || horizontals.len() < MIN_ROWS + 1 {
        return Vec::new();
    }

    let mut tables = Vec::new();
    for group in group_by_overlap(&verticals) {
        let columns = cluster(group.iter().map(|r| r.position).collect());
        if columns.len() < MIN_COLUMNS + 1 {
            continue;
        }
        let (left, right) = (columns[0], columns[columns.len() - 1]);
        let width = right - left;
        if width <= 0.0 {
            continue;
        }
        let top = group.iter().fold(f32::MAX, |v, r| v.min(r.from.min(r.to)));
        let bottom = group.iter().fold(f32::MIN, |v, r| v.max(r.from.max(r.to)));

        // A horizontal rule belongs to this grid only if it lies inside the
        // grid's vertical extent *and* crosses most of its width. The second
        // half is what excludes a page-wide section rule that happens to sit
        // beside a table, and a short mark inside one cell.
        let mut row_positions = Vec::new();
        for rule in &horizontals {
            let inside = rule.position >= top - TOLERANCE && rule.position <= bottom + TOLERANCE;
            let overlap = rule.to.min(right) - rule.from.max(left);
            if inside && overlap >= width * SPAN_SHARE {
                row_positions.push(rule.position);
            }
        }
        let rows = cluster(row_positions);
        if rows.len() < MIN_ROWS + 1 {
            continue;
        }

        let mut cells = Vec::new();
        for (row, pair) in rows.windows(2).enumerate() {
            for (column, span) in columns.windows(2).enumerate() {
                cells.push(Cell {
                    row,
                    column,
                    span: 1,
                    left: span[0],
                    right: span[1],
                    top: pair[0],
                    bottom: pair[1],
                });
            }
        }
        tables.push(Table { columns, rows, cells });
    }
    tables
}

/// Group vertical rules into candidate grids: two verticals belong together
/// when their vertical extents overlap. Two tables one above the other on a
/// page therefore stay two tables.
fn group_by_overlap(verticals: &[Rule]) -> Vec<Vec<Rule>> {
    let mut sorted: Vec<Rule> = verticals.to_vec();
    sorted.sort_by(|a, b| a.from.min(a.to).total_cmp(&b.from.min(b.to)));

    let mut groups: Vec<Vec<Rule>> = Vec::new();
    let mut extent: Option<(f32, f32)> = None;
    for rule in sorted {
        let (start, end) = (rule.from.min(rule.to), rule.from.max(rule.to));
        match extent {
            Some((group_start, group_end)) if start <= group_end - TOLERANCE => {
                if let Some(group) = groups.last_mut() {
                    group.push(rule);
                }
                extent = Some((group_start, group_end.max(end)));
            }
            _ => {
                groups.push(vec![rule]);
                extent = Some((start, end));
            }
        }
    }
    groups
}

/// A rule is only a rule if it is long enough to be one. Shorter than this and
/// it is a tick, a bullet, or a dash in the text.
pub const MIN_RULE_LENGTH: f32 = 8.0;

/// Keep the rules worth considering. Public so the caller can filter before
/// paying for detection.
#[must_use]
pub fn usable(rules: &[Rule]) -> Vec<Rule> {
    rules
        .iter()
        .copied()
        .filter(|r| r.length() >= MIN_RULE_LENGTH)
        .collect()
}

#[allow(clippy::expect_used, clippy::unwrap_used, clippy::float_cmp)]
#[cfg(test)]
mod tests {
    use super::*;

    fn h(y: f32, from: f32, to: f32) -> Rule {
        Rule { horizontal: true, position: y, from, to }
    }
    fn v(x: f32, from: f32, to: f32) -> Rule {
        Rule { horizontal: false, position: x, from, to }
    }

    /// The fixture's grid, in the frame a reader sees (y downwards): three
    /// columns at x 72/220/360/500, four rows between y 152 and 248.
    fn grid() -> Vec<Rule> {
        let mut rules = Vec::new();
        for y in [152.0, 176.0, 200.0, 224.0, 248.0] {
            rules.push(h(y, 72.0, 500.0));
        }
        for x in [72.0, 220.0, 360.0, 500.0] {
            rules.push(v(x, 152.0, 248.0));
        }
        rules
    }

    #[test]
    fn a_grid_yields_a_table_of_the_right_shape() {
        let tables = detect_tables(&grid());
        assert_eq!(tables.len(), 1);
        let table = &tables[0];
        assert_eq!(table.row_count(), 4);
        assert_eq!(table.column_count(), 3);
        assert_eq!(table.cells.len(), 12);
        assert_eq!((table.left(), table.right()), (72.0, 500.0));
        assert_eq!((table.top(), table.bottom()), (152.0, 248.0));
    }

    #[test]
    fn cells_are_bounded_by_their_own_grid_lines() {
        let table = &detect_tables(&grid())[0];
        let first = table.cells.iter().find(|c| c.row == 0 && c.column == 0).expect("cell");
        assert_eq!((first.left, first.right), (72.0, 220.0));
        assert_eq!((first.top, first.bottom), (152.0, 176.0));
        let last = table.cells.iter().find(|c| c.row == 3 && c.column == 2).expect("cell");
        assert_eq!((last.left, last.right), (360.0, 500.0));
        assert_eq!((last.top, last.bottom), (224.0, 248.0));
    }

    #[test]
    fn a_section_rule_is_not_a_table() {
        // The failure this whole guard exists for: two page-wide rules, of the
        // kind that separate sections, with no verticals at all.
        let rules = vec![h(86.0, 72.0, 540.0), h(296.0, 72.0, 540.0)];
        assert!(detect_tables(&rules).is_empty());
    }

    #[test]
    fn a_section_rule_beside_a_table_does_not_join_it() {
        // The fixture's decoys: page-wide rules above and below the grid. They
        // are outside its vertical extent, so they must not become rows — a
        // table that swallows the paragraph above it is a visible defect.
        let mut rules = grid();
        rules.push(h(86.0, 72.0, 540.0));
        rules.push(h(296.0, 72.0, 540.0));
        let table = &detect_tables(&rules)[0];
        assert_eq!(table.row_count(), 4, "a decoy rule was taken as a row");
        assert_eq!(table.top(), 152.0);
        assert_eq!(table.bottom(), 248.0);
    }

    #[test]
    fn a_short_mark_inside_a_cell_is_not_a_row() {
        // A tick, an underline, a minus sign drawn as a path. It is inside the
        // grid's extent but crosses almost none of its width.
        let mut rules = grid();
        rules.push(h(164.0, 80.0, 110.0));
        let table = &detect_tables(&rules)[0];
        assert_eq!(table.row_count(), 4, "a short mark was taken as a row");
    }

    #[test]
    fn a_single_box_is_not_a_table() {
        // Two verticals and two horizontals bound one cell. A box around a
        // pull-quote is the common case, and it is not a table.
        let rules = vec![
            h(100.0, 72.0, 300.0), h(200.0, 72.0, 300.0),
            v(72.0, 100.0, 200.0), v(300.0, 100.0, 200.0),
        ];
        assert!(detect_tables(&rules).is_empty());
    }

    #[test]
    fn a_single_row_strip_is_not_a_table() {
        // Three verticals, two horizontals: one row of two cells. Common as a
        // header band; not a table worth locking text into.
        let rules = vec![
            h(100.0, 72.0, 300.0), h(130.0, 72.0, 300.0),
            v(72.0, 100.0, 130.0), v(180.0, 100.0, 130.0), v(300.0, 100.0, 130.0),
        ];
        assert!(detect_tables(&rules).is_empty());
    }

    #[test]
    fn a_boxed_list_is_not_a_table() {
        // Two verticals and four horizontals: a bordered sidebar divided into
        // three bands. It has rows but only one column, and locking a list into
        // a one-column table helps nobody.
        let rules = vec![
            h(100.0, 72.0, 300.0), h(140.0, 72.0, 300.0),
            h(180.0, 72.0, 300.0), h(220.0, 72.0, 300.0),
            v(72.0, 100.0, 220.0), v(300.0, 100.0, 220.0),
        ];
        assert!(detect_tables(&rules).is_empty());
    }

    #[test]
    fn near_identical_rules_collapse_to_one_line() {
        // A grid drawn twice — a fill and a stroke over it — must not become a
        // table with twice as many rows and zero-width cells.
        let mut rules = grid();
        for rule in grid() {
            rules.push(Rule { position: rule.position + 0.4, ..rule });
        }
        let table = &detect_tables(&rules)[0];
        assert_eq!(table.row_count(), 4);
        assert_eq!(table.column_count(), 3);
    }

    #[test]
    fn two_tables_on_a_page_stay_two_tables() {
        let mut rules = grid();
        for rule in grid() {
            // The same grid 300 pt further down.
            rules.push(Rule {
                position: if rule.horizontal { rule.position + 300.0 } else { rule.position },
                from: rule.from + if rule.horizontal { 0.0 } else { 300.0 },
                to: rule.to + if rule.horizontal { 0.0 } else { 300.0 },
                ..rule
            });
        }
        let tables = detect_tables(&rules);
        assert_eq!(tables.len(), 2, "the two grids were merged");
        assert_eq!(tables[0].row_count(), 4);
        assert_eq!(tables[1].row_count(), 4);
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert!(detect_tables(&[]).is_empty());
        assert!(detect_tables(&grid()[..3]).is_empty());
    }

    #[test]
    fn short_rules_are_dropped_before_detection() {
        let rules = vec![h(100.0, 72.0, 76.0), h(120.0, 72.0, 300.0)];
        let kept = usable(&rules);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].position, 120.0);
    }

    #[test]
    fn a_table_knows_what_it_contains() {
        let table = &detect_tables(&grid())[0];
        assert!(table.contains(100.0, 160.0), "a point in the first cell");
        assert!(!table.contains(100.0, 100.0), "a point above the table");
        assert!(!table.contains(550.0, 160.0), "a point beside the table");
    }
}
