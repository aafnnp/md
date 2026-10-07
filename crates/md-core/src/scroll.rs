//! The arithmetic behind scrolling the two panes together.
//!
//! Kept here rather than in `md-app` so it can be tested without a window: this
//! crate has no `gpui` dependency, and the module is nothing but numbers.
//!
//! The two panes cannot be aligned line for line. The preview reports how far
//! it can scroll, but `EditorState` does not — its scroll extent and its
//! `ScrollHandle` are both `pub(crate)` with no getter — so the editor's is
//! estimated from its line count and line height. That estimate is what makes
//! this a *proportional* follow: the panes travel together, each at the same
//! fraction of its own length, rather than row by row.

/// How far into a scrollable range an offset sits, from `0.0` to `1.0`.
///
/// `span` is the whole distance the pane can scroll: its content height less
/// its viewport height. A pane with nothing to scroll has no fraction, and
/// reports `0.0` rather than dividing by zero into a `NaN` that would then be
/// handed on as a scroll position.
///
/// `offset` is read as a magnitude, so a GPUI offset can be passed straight in
/// even though it is negative once scrolled down.
pub fn fraction(offset: f32, span: f32) -> f32 {
    if !span.is_finite() || span <= 0.0 || !offset.is_finite() {
        return 0.0;
    }
    (offset.abs() / span).clamp(0.0, 1.0)
}

/// Where a follower pane goes to sit at the same `fraction` of its own range.
///
/// `max` is the follower's own scrollable distance — the source's extent says
/// nothing about it, and only the proportion carries across.
pub fn target(max: f32, fraction: f32) -> f32 {
    if !max.is_finite() || max <= 0.0 || !fraction.is_finite() {
        return 0.0;
    }
    max * fraction.clamp(0.0, 1.0)
}

/// The editor's scrollable distance, estimated.
///
/// `lines × line_height − viewport` is as close as the public API gets. Soft
/// wrap and folded regions make the real content taller than this counts, so
/// the estimate runs short and the preview reaches the end a little before the
/// source does. That is the whole of the imprecision; nothing here is wrong so
/// much as it is approximate.
pub fn extent(lines: usize, line_height: f32, viewport: f32) -> f32 {
    if !line_height.is_finite() || !viewport.is_finite() {
        return 0.0;
    }
    (lines as f32 * line_height - viewport).max(0.0)
}

/// The number of lines in `text`: one more than it has newlines.
///
/// The source pane's line count is needed on every scroll frame, and the rope
/// it lives in cannot be asked directly — its `len_lines` takes a feature-gated
/// `LineType` that would mean adding `ropey` as a dependency of `md-app` to
/// name. Counting newlines is the same answer, over text the tab already has.
pub fn line_count(text: &str) -> usize {
    text.matches('\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_top_is_zero_and_the_bottom_is_one() {
        assert_eq!(fraction(0.0, 500.0), 0.0);
        assert_eq!(fraction(250.0, 500.0), 0.5);
        assert_eq!(fraction(500.0, 500.0), 1.0);
    }

    #[test]
    fn a_scroll_offset_is_read_as_a_magnitude() {
        // GPUI reports "scrolled down 250" as -250. Which way round it is
        // cannot change how far down the pane is.
        assert_eq!(fraction(-250.0, 500.0), fraction(250.0, 500.0));
    }

    #[test]
    fn a_document_with_nothing_to_scroll_has_no_fraction() {
        assert_eq!(fraction(100.0, 0.0), 0.0);
        assert_eq!(fraction(100.0, -20.0), 0.0);
        // Neither a NaN offset nor a NaN span may reach a scroll position.
        assert_eq!(fraction(f32::NAN, 500.0), 0.0);
        assert_eq!(fraction(100.0, f32::NAN), 0.0);
    }

    #[test]
    fn an_offset_past_the_end_is_clamped() {
        assert_eq!(fraction(900.0, 500.0), 1.0);
        assert_eq!(fraction(-900.0, 500.0), 1.0);
    }

    #[test]
    fn a_fraction_maps_onto_the_follower_by_proportion() {
        assert_eq!(target(1000.0, 0.0), 0.0);
        assert_eq!(target(1000.0, 0.25), 250.0);
        assert_eq!(target(1000.0, 1.0), 1000.0);
    }

    #[test]
    fn a_follower_with_nothing_to_scroll_stays_put() {
        assert_eq!(target(0.0, 0.8), 0.0);
        assert_eq!(target(-5.0, 0.8), 0.0);
        assert_eq!(target(f32::NAN, 0.8), 0.0);
        assert_eq!(target(1000.0, f32::NAN), 0.0);
    }

    #[test]
    fn the_estimated_extent_is_never_negative() {
        // A single line in a tall window has nowhere to scroll.
        assert_eq!(extent(1, 20.0, 800.0), 0.0);
        assert_eq!(extent(100, 20.0, 800.0), 1200.0);
        assert_eq!(extent(0, 20.0, 800.0), 0.0);
        assert_eq!(extent(100, f32::NAN, 800.0), 0.0);
        assert_eq!(extent(100, 20.0, f32::NAN), 0.0);
    }

    #[test]
    fn a_text_has_one_more_line_than_it_has_newlines() {
        assert_eq!(line_count(""), 1);
        assert_eq!(line_count("one line"), 1);
        assert_eq!(line_count("a\nb"), 2);
        // A trailing newline opens a line the caret can sit on.
        assert_eq!(line_count("a\nb\n"), 3);
    }
}
