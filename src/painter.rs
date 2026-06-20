//! Cell processing: terminal cells → text runs + background rectangles.
//!
//! Handles inverse swap, APCA contrast, dim, batching, and background merging.

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use gpui::{
    Font, FontFeatures, FontStyle, FontWeight, Hsla, Pixels, SharedString, StrikethroughStyle,
    UnderlineStyle, px,
};

use crate::colors::{cell_bg, cell_fg, is_explicit_color};
use crate::contrast;

// =========================================================================
// Layout Types
// =========================================================================

/// A position in terminal grid coordinates (line, column).
#[derive(Clone, Copy, Debug)]
pub struct LayoutPoint {
    pub line: i32,
    pub column: i32,
}

impl LayoutPoint {
    pub fn new(line: i32, column: i32) -> Self {
        Self { line, column }
    }
}

/// A single-line background rectangle to paint.
#[derive(Clone, Debug)]
pub struct LayoutRect {
    pub point: LayoutPoint,
    pub num_of_cells: usize,
    pub color: Hsla,
}

impl LayoutRect {
    pub fn new(point: LayoutPoint, num_of_cells: usize, color: Hsla) -> Self {
        Self {
            point,
            num_of_cells,
            color,
        }
    }
}

/// A potentially multi-line background region for merging.
#[derive(Debug, Clone)]
struct BackgroundRegion {
    start_line: i32,
    start_col: i32,
    end_line: i32,
    end_col: i32,
    color: Hsla,
}

impl BackgroundRegion {
    fn new(line: i32, col: i32, color: Hsla) -> Self {
        Self {
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col,
            color,
        }
    }
}

/// A batched run of text with the same style for efficient shaping.
#[derive(Clone, Debug)]
pub struct BatchedTextRun {
    pub start_point: LayoutPoint,
    pub text: SharedString,
    pub cell_count: usize,
    pub style: gpui::TextRun,
    pub font_size: Pixels,
}

impl BatchedTextRun {
    pub fn new_from_char(
        start_point: LayoutPoint,
        ch: char,
        style: gpui::TextRun,
        font_size: Pixels,
    ) -> Self {
        Self {
            start_point,
            text: SharedString::from(String::from(ch)),
            cell_count: 1,
            style,
            font_size,
        }
    }

    pub fn can_append(&self, other: &gpui::TextRun) -> bool {
        self.style.font == other.font
            && self.style.color == other.color
            && self.style.background_color == other.background_color
            && self.style.underline == other.underline
            && self.style.strikethrough == other.strikethrough
    }

    pub fn append_char(&mut self, ch: char) {
        let mut s = String::with_capacity(self.text.len() + 1);
        s.push_str(&self.text);
        s.push(ch);
        self.text = SharedString::from(s);
        self.cell_count += 1;
        self.style.len = self.text.len();
    }
}

// =========================================================================
// Cell Processing
// =========================================================================

/// Alpha multiplier for dimmed cells.
const DIM_ALPHA_FACTOR: f32 = 0.7;

/// Single-pass cell processing: terminal cells → text runs + background rects.
///
/// Handles inverse swap, APCA contrast, dim, batching, and background region
/// collection. Cells are iterated in row-major order using index arithmetic.
#[allow(clippy::too_many_arguments)]
pub fn process_cells(
    cells: &[(alacritty_terminal::index::Point, alacritty_terminal::term::cell::Cell)],
    cols: usize,
    rows: usize,
    tc: &crate::colors::TerminalColors,
    font_family: &SharedString,
    font_size_px: Pixels,
    default_fg: Hsla,
    decorative_check: impl Fn(char) -> bool,
) -> (Vec<BatchedTextRun>, Vec<LayoutRect>) {
    let estimated_cells = rows * cols;
    let mut background_regions: Vec<BackgroundRegion> = Vec::with_capacity(estimated_cells / 20);
    let mut runs: Vec<BatchedTextRun> = Vec::with_capacity(rows * 2);

    // Memoize APCA contrast adjustments: the same (fg, bg) pair recurs across
    // many cells, and the adjustment is a costly binary search.
    let mut contrast_cache: std::collections::HashMap<[u32; 8], Hsla> =
        std::collections::HashMap::new();

    let mut current_batch: Option<BatchedTextRun> = None;
    let mut prev_line: i32 = -1;

    for (idx, (_point, cell)) in cells.iter().enumerate() {
        let l = idx / cols;
        let c = idx % cols;
        if l >= rows || c >= cols {
            continue;
        }
        let vp_line = l as i32;

        if vp_line != prev_line {
            if let Some(batch) = current_batch.take() {
                runs.push(batch);
            }
            prev_line = vp_line;
        }

        let mut fg_color = cell.fg;
        let mut bg_color = cell.bg;
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg_color, &mut bg_color);
        }

        // Convert to Hsla
        let mut fg = match fg_color {
            Color::Named(NamedColor::Foreground) => None,
            o => Some(cell_fg(o, tc)),
        };
        let bg = match bg_color {
            Color::Named(NamedColor::Background) => None,
            o => Some(cell_bg(o, tc)),
        };

        // APCA contrast for explicit fg+bg pairs
        if let (Some(f), Some(b)) = (&mut fg, bg) {
            if !is_explicit_color(fg_color) && !decorative_check(cell.c) {
                let key = [
                    f.h.to_bits(),
                    f.s.to_bits(),
                    f.l.to_bits(),
                    f.a.to_bits(),
                    b.h.to_bits(),
                    b.s.to_bits(),
                    b.l.to_bits(),
                    b.a.to_bits(),
                ];
                *f = *contrast_cache.entry(key).or_insert_with(|| {
                    contrast::ensure_minimum_contrast(*f, b, contrast::MINIMUM_CONTRAST)
                });
            }
        }

        // Dim foreground
        if cell.flags.contains(Flags::DIM) {
            fg = fg.map(|mut c| {
                c.a *= DIM_ALPHA_FACTOR;
                c
            });
        }

        // Collect background region (in-place horizontal merge on same line)
        if let Some(bg_color) = bg {
            let col = c as i32;
            if let Some(last) = background_regions.last_mut() {
                if last.color == bg_color
                    && last.start_line == vp_line
                    && last.end_line == vp_line
                    && last.end_col + 1 == col
                {
                    last.end_col = col;
                } else {
                    background_regions.push(BackgroundRegion::new(vp_line, col, bg_color));
                }
            } else {
                background_regions.push(BackgroundRegion::new(vp_line, col, bg_color));
            }
        }

        // Build character for text run — skip hidden cells
        if cell.flags.contains(Flags::HIDDEN) {
            continue;
        }
        let ch = if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
            || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
        {
            '\u{00A0}'
        } else {
            let c = cell.c;
            if c == ' ' {
                '\u{00A0}'
            } else {
                c
            }
        };

        let fc = fg.unwrap_or(default_fg);
        let has_hyperlink = cell.hyperlink().is_some();
        let underline = if cell.flags.intersects(Flags::ALL_UNDERLINES) || has_hyperlink {
            let wavy = cell.flags.contains(Flags::UNDERCURL);
            let thickness = if cell.flags.contains(Flags::DOUBLE_UNDERLINE) {
                px(2.0)
            } else {
                px(1.0)
            };
            Some(UnderlineStyle {
                color: Some(fc),
                thickness,
                wavy,
            })
        } else {
            None
        };
        let strikethrough = cell.flags.contains(Flags::STRIKEOUT).then(|| StrikethroughStyle {
            color: Some(fc),
            thickness: px(1.0),
        });
        let weight = if cell.flags.contains(Flags::BOLD) {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };
        let font_style = if cell.flags.contains(Flags::ITALIC) {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };

        let fc = if cell.flags.contains(Flags::DIM) && fg.is_none() {
            let mut c = fc;
            c.a *= DIM_ALPHA_FACTOR;
            c
        } else {
            fc
        };

        let t_run = gpui::TextRun {
            len: ch.len_utf8(),
            font: Font {
                family: font_family.clone(),
                weight,
                style: font_style,
                features: FontFeatures::default(),
                fallbacks: None,
            },
            color: fc,
            background_color: None,
            underline,
            strikethrough,
        };

        let cell_point = LayoutPoint::new(vp_line, c as i32);
        if let Some(ref mut batch) = current_batch {
            if batch.can_append(&t_run)
                && batch.start_point.line == cell_point.line
                && batch.start_point.column + batch.cell_count as i32 == cell_point.column
            {
                batch.append_char(ch);
            } else {
                runs.push(current_batch.take().unwrap());
                current_batch = Some(BatchedTextRun::new_from_char(
                    cell_point,
                    ch,
                    t_run,
                    font_size_px,
                ));
            }
        } else {
            current_batch = Some(BatchedTextRun::new_from_char(
                cell_point,
                ch,
                t_run,
                font_size_px,
            ));
        }
    }

    if let Some(batch) = current_batch.take() {
        runs.push(batch);
    }

    // 2D merge of background regions
    let merged_regions = merge_background_regions(background_regions);
    let mut rects = Vec::new();
    for region in merged_regions {
        for line in region.start_line..=region.end_line {
            rects.push(LayoutRect::new(
                LayoutPoint::new(line, region.start_col),
                (region.end_col - region.start_col + 1) as usize,
                region.color,
            ));
        }
    }

    (runs, rects)
}

// =========================================================================
// Background Region Merging — minimize GPU paint calls
// =========================================================================

/// Merge regions in 2D to minimize GPU paint calls.
///
/// Pass 1: horizontal merge on identical lines.
/// Pass 2: vertical merge — same column span, adjacent lines.
fn merge_background_regions(regions: Vec<BackgroundRegion>) -> Vec<BackgroundRegion> {
    if regions.len() <= 1 {
        return regions;
    }

    let mut color_groups: std::collections::HashMap<Hsla, Vec<BackgroundRegion>> =
        std::collections::HashMap::new();
    for r in regions {
        color_groups.entry(r.color).or_default().push(r);
    }
    let mut result = Vec::new();

    for (_, mut group) in color_groups {
        if group.len() == 1 {
            result.push(group.pop().unwrap());
            continue;
        }
        group.sort_by_key(|r| (r.start_line, r.start_col));

        let mut horiz_merged: Vec<BackgroundRegion> = Vec::with_capacity(group.len());
        for r in group {
            if let Some(last) = horiz_merged.last_mut() {
                if last.start_line == r.start_line
                    && last.end_line == r.end_line
                    && last.end_col + 1 >= r.start_col
                {
                    last.end_col = last.end_col.max(r.end_col);
                    continue;
                }
            }
            horiz_merged.push(r);
        }

        let mut span_map: std::collections::HashMap<(i32, i32), usize> =
            std::collections::HashMap::with_capacity(horiz_merged.len());
        let mut vert_merged: Vec<BackgroundRegion> = Vec::with_capacity(horiz_merged.len());
        for r in horiz_merged {
            let span_key = (r.start_col, r.end_col);
            if let Some(&idx) = span_map.get(&span_key) {
                let prev = &mut vert_merged[idx];
                if prev.end_line + 1 == r.start_line {
                    prev.end_line = r.end_line;
                    prev.start_col = prev.start_col.min(r.start_col);
                    prev.end_col = prev.end_col.max(r.end_col);
                    continue;
                }
            }
            span_map.insert(span_key, vert_merged.len());
            vert_merged.push(r);
        }
        result.extend(vert_merged);
    }
    result
}

#[cfg(test)]
mod tests {
    use gpui::Hsla;
    use super::{BackgroundRegion, merge_background_regions};

    fn make_region(line: i32, col_start: i32, col_end: i32) -> BackgroundRegion {
        BackgroundRegion {
            start_line: line,
            start_col: col_start,
            end_line: line,
            end_col: col_end,
            color: Hsla::default(),
        }
    }

    #[test]
    fn merge_three_adjacent_horizontal() {
        let regions = vec![
            make_region(0, 0, 2),
            make_region(0, 3, 5),
            make_region(0, 6, 8),
        ];
        let result = merge_background_regions(regions);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start_line, 0);
        assert_eq!(result[0].end_col, 8);
    }

    #[test]
    fn merge_different_colors_no_merge() {
        let r1 = BackgroundRegion {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 2,
            color: Hsla::default(),
        };
        let r2 = BackgroundRegion {
            start_line: 0,
            start_col: 3,
            end_line: 0,
            end_col: 5,
            color: Hsla {
                h: 0.5,
                s: 0.5,
                l: 0.5,
                a: 1.0,
            },
        };
        let result = merge_background_regions(vec![r1, r2]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn merge_vertical_same_span() {
        let regions = vec![
            make_region(0, 2, 5),
            make_region(1, 2, 5),
            make_region(2, 2, 5),
        ];
        let result = merge_background_regions(regions);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start_line, 0);
        assert_eq!(result[0].end_line, 2);
    }

    #[test]
    fn empty_regions() {
        assert!(merge_background_regions(vec![]).is_empty());
    }
}
