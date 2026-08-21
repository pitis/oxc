//! CSS `display` and `white-space`, which is what decides where a template
//! may break.
//!
//! HTML layout follows from the stylesheet the browser applies before any of
//! the page's own CSS: whitespace around a `<div>` never shows, whitespace
//! around a `<span>` always does. A printer that moved a `<span>` onto a line
//! of its own would add a space the page renders, so the display category has
//! to be known before any layout decision is taken.
//!
//! The two tables are the ones Prettier derives from `html-ua-styles`, which
//! is the WHATWG rendering section expressed as a stylesheet.

use crate::options::WhitespaceSensitivity;

/// A CSS `display` value, in the terms the layout decisions are taken in.
///
/// `Other` stands for any value this printer does not need to distinguish —
/// it can only arrive through a `<!-- display: … -->` comment, and every
/// predicate below treats an unrecognised value exactly as Prettier's string
/// comparisons do: not block-like, not `inline-block`, not a table part.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Display {
    None,
    Block,
    #[default]
    Inline,
    InlineBlock,
    ListItem,
    Contents,
    Ruby,
    RubyText,
    Table,
    TableCaption,
    TableColumnGroup,
    TableColumn,
    TableHeaderGroup,
    TableRowGroup,
    TableFooterGroup,
    TableRow,
    TableCell,
    Other,
}

impl Display {
    /// Whether whitespace around the element collapses away entirely, which is
    /// what lets the printer put it on a line of its own. Prettier's
    /// `isBlockLikeCssDisplay`.
    pub fn is_block_like(self) -> bool {
        matches!(self, Self::Block | Self::ListItem) || self.is_table_part()
    }

    /// Every `display` whose name begins with `table`, which CSS treats as one
    /// family for whitespace purposes.
    pub fn is_table_part(self) -> bool {
        matches!(
            self,
            Self::Table
                | Self::TableCaption
                | Self::TableColumnGroup
                | Self::TableColumn
                | Self::TableHeaderGroup
                | Self::TableRowGroup
                | Self::TableFooterGroup
                | Self::TableRow
                | Self::TableCell
        )
    }

    /// Whether the *first* child's leading whitespace shows. Prettier's
    /// `isFirstChildLeadingSpaceSensitiveCssDisplay`, which is also
    /// `isLastChildTrailingSpaceSensitiveCssDisplay` and
    /// `isDanglingSpaceSensitiveCssDisplay` — three names for one test.
    pub fn is_edge_space_sensitive(self) -> bool {
        !self.is_block_like() && self != Self::InlineBlock
    }

    /// Whether whitespace *between* this element and its neighbour shows.
    /// Prettier's `isPrevTrailingSpaceSensitiveCssDisplay` /
    /// `isNextLeadingSpaceSensitiveCssDisplay`.
    pub fn is_between_space_sensitive(self) -> bool {
        !self.is_block_like()
    }

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "none" => Self::None,
            "block" => Self::Block,
            "inline" => Self::Inline,
            "inline-block" => Self::InlineBlock,
            "list-item" => Self::ListItem,
            "contents" => Self::Contents,
            "ruby" => Self::Ruby,
            "ruby-text" => Self::RubyText,
            "table" => Self::Table,
            "table-caption" => Self::TableCaption,
            "table-column-group" => Self::TableColumnGroup,
            "table-column" => Self::TableColumn,
            "table-header-group" => Self::TableHeaderGroup,
            "table-row-group" => Self::TableRowGroup,
            "table-footer-group" => Self::TableFooterGroup,
            "table-row" => Self::TableRow,
            "table-cell" => Self::TableCell,
            _ => return None,
        })
    }

    /// The value named by a `<!-- display: … -->` comment, which overrides the
    /// element's own. An unknown name is still an override — it just does not
    /// match any of the categories the layout tests for.
    pub fn from_comment_name(name: &str) -> Self {
        Self::from_name(name).unwrap_or(Self::Other)
    }
}

/// A CSS `white-space` value, of which only "is it `pre`-like" is ever asked.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Pre,
    PreWrap,
    NoWrap,
    Initial,
}

impl WhiteSpace {
    /// Whether the element renders its own whitespace, so its content is not
    /// the printer's to lay out. Prettier tests `startsWith("pre")`, which is
    /// `pre` and `pre-wrap`.
    pub fn is_pre_like(self) -> bool {
        matches!(self, Self::Pre | Self::PreWrap)
    }
}

/// The user-agent stylesheet's `display`, for the elements that have one.
///
/// Everything absent from this table is `inline`, which is
/// [`Display::default()`]. The five entries at the end are Prettier's own
/// additions: elements the stylesheet gives `display: none` but which behave
/// as content, and the replaced elements CSS gives no display at all.
static CSS_DISPLAY_TAGS: &[(&str, Display)] = &[
    ("area", Display::None),
    ("base", Display::None),
    ("basefont", Display::None),
    ("datalist", Display::None),
    ("head", Display::None),
    ("link", Display::None),
    ("meta", Display::None),
    ("noembed", Display::None),
    ("noframes", Display::None),
    ("param", Display::Block),
    ("rp", Display::None),
    ("script", Display::Block),
    ("style", Display::None),
    ("template", Display::Inline),
    ("title", Display::None),
    ("html", Display::Block),
    ("body", Display::Block),
    ("address", Display::Block),
    ("blockquote", Display::Block),
    ("center", Display::Block),
    ("dialog", Display::Block),
    ("div", Display::Block),
    ("figure", Display::Block),
    ("figcaption", Display::Block),
    ("footer", Display::Block),
    ("form", Display::Block),
    ("header", Display::Block),
    ("hr", Display::Block),
    ("legend", Display::Block),
    ("listing", Display::Block),
    ("main", Display::Block),
    ("p", Display::Block),
    ("plaintext", Display::Block),
    ("pre", Display::Block),
    ("search", Display::Block),
    ("xmp", Display::Block),
    ("slot", Display::Contents),
    ("ruby", Display::Ruby),
    ("rt", Display::RubyText),
    ("article", Display::Block),
    ("aside", Display::Block),
    ("h1", Display::Block),
    ("h2", Display::Block),
    ("h3", Display::Block),
    ("h4", Display::Block),
    ("h5", Display::Block),
    ("h6", Display::Block),
    ("hgroup", Display::Block),
    ("nav", Display::Block),
    ("section", Display::Block),
    ("dir", Display::Block),
    ("dd", Display::Block),
    ("dl", Display::Block),
    ("dt", Display::Block),
    ("menu", Display::Block),
    ("ol", Display::Block),
    ("ul", Display::Block),
    ("li", Display::ListItem),
    ("table", Display::Table),
    ("caption", Display::TableCaption),
    ("colgroup", Display::TableColumnGroup),
    ("col", Display::TableColumn),
    ("thead", Display::TableHeaderGroup),
    ("tbody", Display::TableRowGroup),
    ("tfoot", Display::TableFooterGroup),
    ("tr", Display::TableRow),
    ("td", Display::TableCell),
    ("th", Display::TableCell),
    ("input", Display::InlineBlock),
    ("button", Display::InlineBlock),
    ("fieldset", Display::Block),
    ("details", Display::Block),
    ("summary", Display::Block),
    ("marquee", Display::InlineBlock),
    ("option", Display::Block),
    ("optgroup", Display::Block),
    ("select", Display::InlineBlock),
    ("source", Display::Block),
    ("track", Display::Block),
    ("meter", Display::InlineBlock),
    ("progress", Display::InlineBlock),
    ("object", Display::InlineBlock),
    ("video", Display::InlineBlock),
    ("audio", Display::InlineBlock),
];

/// The user-agent stylesheet's `white-space`, for the seven elements that
/// deviate from `normal`.
static CSS_WHITE_SPACE_TAGS: &[(&str, WhiteSpace)] = &[
    ("listing", WhiteSpace::Pre),
    ("plaintext", WhiteSpace::Pre),
    ("pre", WhiteSpace::Pre),
    ("xmp", WhiteSpace::Pre),
    ("nobr", WhiteSpace::NoWrap),
    ("table", WhiteSpace::Initial),
    ("textarea", WhiteSpace::PreWrap),
];

/// The `display` an element name carries, or `None` when the stylesheet says
/// nothing about it — which includes every component name, since a Vue
/// template's tag names are case-sensitive and a component is not an HTML
/// element.
pub fn element_display(name: &str) -> Option<Display> {
    CSS_DISPLAY_TAGS.iter().find(|(tag, _)| *tag == name).map(|(_, display)| *display)
}

pub fn element_white_space(name: &str) -> WhiteSpace {
    CSS_WHITE_SPACE_TAGS
        .iter()
        .find(|(tag, _)| *tag == name)
        .map_or(WhiteSpace::Normal, |(_, value)| *value)
}

/// The `display` a whitespace sensitivity setting forces regardless of the
/// element, or `None` when the stylesheet decides.
///
/// `strict` calls everything inline, so no element's surrounding whitespace
/// may be moved; `ignore` calls everything a block, so all of it may be.
pub fn forced_display(sensitivity: WhitespaceSensitivity) -> Option<Display> {
    match sensitivity {
        WhitespaceSensitivity::Strict => Some(Display::Inline),
        WhitespaceSensitivity::Ignore => Some(Display::Block),
        WhitespaceSensitivity::Css => None,
    }
}

/// Elements whose first line break is part of the syntax rather than the
/// content, so the parser is required to drop it. Writing `<pre>` and its
/// content on the next line is the normal spelling, and the newline that
/// spelling introduces is not text.
pub fn ignores_first_line_feed(name: &str) -> bool {
    matches!(name, "pre" | "textarea" | "listing")
}

/// The whitespace HTML collapses: space, tab, form feed, CR, LF.
pub fn is_collapsible_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

pub fn is_only_collapsible_whitespace(text: &str) -> bool {
    text.bytes().all(is_collapsible_whitespace)
}

pub fn has_collapsible_whitespace(text: &str) -> bool {
    text.bytes().any(is_collapsible_whitespace)
}

/// The run of collapsible whitespace at the start of `text`.
pub fn leading_whitespace(text: &str) -> &str {
    let end = text.bytes().position(|byte| !is_collapsible_whitespace(byte)).unwrap_or(text.len());
    &text[..end]
}

/// The run of collapsible whitespace at the end of `text`.
pub fn trailing_whitespace(text: &str) -> &str {
    let start = text
        .bytes()
        .rposition(|byte| !is_collapsible_whitespace(byte))
        .map_or(0, |index| index + 1);
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::{Display, WhiteSpace, element_display, element_white_space};

    #[test]
    fn display_categories_match_the_stylesheet() {
        assert_eq!(element_display("div"), Some(Display::Block));
        assert_eq!(element_display("span"), None);
        assert_eq!(element_display("li"), Some(Display::ListItem));
        assert_eq!(element_display("td"), Some(Display::TableCell));
        // A component name is not an element name, and the lookup is
        // case-sensitive, so `<Link>` is not the void `<link>`.
        assert_eq!(element_display("Link"), None);
        assert_eq!(element_display("link"), Some(Display::None));
    }

    #[test]
    fn table_parts_are_block_like_but_only_some_force_a_break() {
        assert!(Display::TableCell.is_block_like());
        assert!(Display::TableCell.is_table_part());
        assert!(!Display::Inline.is_block_like());
        assert!(!Display::InlineBlock.is_block_like());
        // `inline-block` is not block-like, yet its edges are insensitive.
        assert!(!Display::InlineBlock.is_edge_space_sensitive());
        assert!(Display::InlineBlock.is_between_space_sensitive());
    }

    #[test]
    fn pre_like_white_space() {
        assert!(element_white_space("pre").is_pre_like());
        assert!(element_white_space("textarea").is_pre_like());
        assert!(!element_white_space("nobr").is_pre_like());
        assert_eq!(element_white_space("div"), WhiteSpace::Normal);
    }
}
