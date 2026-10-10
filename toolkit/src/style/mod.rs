//! The style engine: sheets of CSS, the element tree's nodes, computed
//! styles. See TOOLKIT.md, "The style engine".
//!
//! A [`Sheet`] is loaded once from its sources for one [`Scheme`]; a
//! view builds the [`Node`]s of its widgets from a surface's root
//! ([`Sheet::root`]); a widget takes its style at rest with
//! [`Node::base`], in other states with [`Node::resolve`]. Styles are
//! resolved once per distinct node and states, and shared.

mod cascade;
mod computed;
mod fonts;
mod images;
mod node;
mod parse;
mod selector;
mod sheet;
#[cfg(test)]
mod tests;
mod value;

pub use computed::{Computed, InsetShadow, Shadow, Sides};
pub use images::{Drawn, Nine, Picture};
pub use node::{Ancestors, Node, Sensitivity, States};
pub use sheet::{Error, Scheme, Sheet, Source, Stats, Warning};
pub use value::{
    Align, BackgroundSize, Corner, Direction, EdgeOver, EdgeUnder, FamilyName, FontStyle, Justify,
    LineHeight, MinSize, ObjectFit, Origin, Repeat, Side, Size, TextAlign, TextBoxEdge,
    TextBoxTrim, TextOverflow, TextTransform, Transform, WhiteSpace,
};
