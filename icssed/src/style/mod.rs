//! The style engine: sheets of CSS, the element tree's nodes, computed
//! styles. See the crate's ARCHITECTURE.md, "The style engine".
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
mod keyframes;
mod node;
mod parse;
mod selector;
mod sheet;
#[cfg(test)]
mod tests;
mod value;

pub use computed::{Animations, Computed, InsetShadow, Shadow, Sides, Transitions};
pub use images::{Drawn, Nine, Picture};
pub use keyframes::Keyframes;
pub use node::{Ancestors, Node, Sensitivity, States};
pub(crate) use node::{Name, NodeId, Step, name};
pub use sheet::{Error, Scheme, Sheet, Source, Stats, Warning};
pub(crate) use value::Moving;
pub use value::{
    Align, AnimationDirection, BackgroundSize, Corner, Direction, Easing, EdgeOver, EdgeUnder,
    FamilyName, FillMode, FontStyle, Justify, LengthPercentage, LineHeight, MinSize, ObjectFit,
    Repeat, Side, Size, StepPosition, TextAlign, TextBoxEdge, TextBoxTrim, TextOverflow,
    TextTransform, Transform, WhiteSpace,
};
