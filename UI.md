# UI features

What the UI needs from iced and from our theme engine: what we have and
what is missing, the starting point for rethinking how we use iced.

Legend: ✅ there · 🟡 there, but limited or worked around · 🔲 missing.
"Needed?": yes / no / ? (to decide).

## Rules

- **A toolkit of its own**: a set of reusable widgets, styled by CSS,
  built on iced. Completely separate from Aria (its own crate): Aria
  uses it, it knows nothing of Aria (no config, services, locale or
  texts of the shell).
- **Performance matters**: styles resolved once, not at every frame; no
  allocations while drawing; layout only when something changed; an
  idle bar costs nothing (no frame without a change); an animation asks
  for frames only while it runs. Measured, before and after a change.
- **No workarounds of any kind**: what iced can't do cleanly is a
  proper widget of the toolkit, or a limit written here; never a trick
  in a view.
- **One source of truth for sizes**: a widget's size comes from its own
  layout, never measured again by hand somewhere else.
- **CSS means CSS**: a property has its CSS name, units and behaviour;
  where ours differs, it's written in the reference, or it gets another
  name.
- **Everything is styleable and documented**: no colour, size or font
  in the code of a widget or a view; every element, class and property
  has its line in the reference, and a neutral default in the
  toolkit's base stylesheet.
- **No `Space` fillers**: in a row, the child that takes the free room
  has `width: fill` (as `flex-grow: 1` does), set in the theme.
- **Every widget is in the gallery and tested**: the toolkit's own
  showcase shows each one in each state; layout and style are unit
  tested without a compositor.
- **Vanilla iced, public API only**: no fork; anything vendored is
  temporary and marked.

## 1. Layout and box

| Feature | State | Needed? |
|---|---|---|
| padding (per side too) | ✅ | |
| border (width, colour, radius, per side) | 🟡 sides that differ take no radius | |
| gap between children | ✅ | |
| width / height: px, auto, fill | ✅ | |
| box-sizing border-box | ✅ | |
| min-width / min-height | 🔲 (min-height on the panel only) | yes |
| max-width / max-height | 🔲 | yes |
| margin | 🔲 | yes |
| alignment of a box's content | 🟡 automatic (centred), not configurable | yes |
| justify-content / align-self / flex-grow | 🔲 (`width: fill` on the child that stretches, see Rules) | no |
| `%` / `em` units | 🔲 | no |
| `fill` inside a box that shrinks to its content | 🟡 grows to the maximum | |

## 2. Box painting

| Feature | State | Needed? |
|---|---|---|
| colour / transparent background | ✅ | |
| borders sharp on the pixel grid | ✅ | |
| box-shadow | ✅ (a surface grows around its root's, not to cut it) | |
| inset box-shadow | ✅ (unblurred) | |
| transparent / rounded surfaces | ✅ | |
| opacity | 🔲 | yes |
| gradients | 🔲 | no |
| background-image / 9-patch | 🔲 | yes |
| outline | 🔲 | yes |

## 3. Text

| Feature | State | Needed? |
|---|---|---|
| font-family with per-glyph fallback, embedded font | ✅ | |
| font-size, font-weight, inherited colour | ✅ | |
| line-height | 🟡 needs the pixel parity | |
| tabular digits | ✅ | |
| word wrapping | ✅ | |
| vertically centred on its letters | 🔲 (1-2 px low) | yes |
| italic | 🔲 | yes |
| text-align | 🔲 | yes |
| ellipsis (`text-overflow`) | 🔲 | yes |
| text-transform | 🔲 | yes |
| letter-spacing | 🔲 | no |

## 4. Icons and images

| Feature | State | Needed? |
|---|---|---|
| icons from the system theme, svg and png | ✅ | |
| png icons sharp on a HiDPI output | 🟡 the theme's scale-1 directories only: scaled up, blurred | yes |
| symbolic icons recoloured by `color` | 🟡 colour fixed in `view`: no hover / disabled | yes |
| size from `height` | ✅ | |
| placeholder for an icon not found yet | 🟡 a `Space` the icon's size, in every view | yes |
| images (cover art, round avatar, wallpaper) | ✅ | |

## 5. States and interaction

| Feature | State | Needed? |
|---|---|---|
| `:hover` / `:active` on buttons, `:focus` on inputs | ✅ | |
| `:disabled` | 🟡 icons need `disabled_if` | yes |
| `:hover` on any element | 🟡 a `mouse_area` case by case | no |
| an ancestor's state styling its descendants | 🟡 only the inherited text colour | no |
| right and middle click, wheel | ✅ | |
| keyboard (launcher, exiter, popups) | ✅ per component | |
| keyboard focus across widgets (Tab, `:focus` on any widget) | 🔲 (iced: only the text input takes focus) | ? |
| accessibility (screen readers, AT-SPI) | 🔲 (none in iced 0.14) | ? |
| tooltips | 🔲 (the tray's tooltip data is there, no widget) | yes |

## 6. Animation

| Feature | State | Needed? |
|---|---|---|
| transitions (hover, `.active`, ...) | 🔲 | yes |
| show / hide animations (OSD, toasts, popups) | 🔲 | yes |
| keyframes | 🔲 | yes, something simple |

## 7. Widgets (the toolkit's pieces)

| Feature | State | Needed? |
|---|---|---|
| button family: base, flat, primary, danger, active, plain, radio, tab; the bar's button | ✅ | |
| toggle (switch), slider, text input with peek | ✅ | |
| menu (checks, radios, submenus, separators) | ✅ | |
| calendar, tabs | ✅ | |
| calendar without Aria's `Locale` (the app gives the day and month names) | 🟡 uses Aria's `Locale` | yes |
| meter, gauge, graphs (sparkline) | ✅ | |
| scrolling lists | 🟡 the scrollbar isn't themed | |
| popup, toast, dialog | ✅ | |
| tooltip | 🔲 | yes |
| select / dropdown | 🔲 | yes, but later |

## 8. Theme system

| Feature | State | Needed? |
|---|---|---|
| selectors: type, class, id, attribute, `*`, descendant, child | ✅ | |
| pseudo-classes: hover, active, focus, disabled, first/last/nth-child, root | ✅ | |
| specificity and source order | ✅ | |
| variables with fallback | ✅ | |
| light / dark schemes | ✅ | |
| theme and scheme switched at runtime, reload when the file changes | ✅ | |
| `@media` / other @-rules, `!important` | 🔲 | no |

## 9. Surfaces

| Feature | State | Needed? |
|---|---|---|
| layer-shell bar per output, anchored popups, dialogs, lock screen, OSD, toasts, wallpaper, screenshot picker | ✅ | |
| room around a surface for its shadow | ✅ | |
| HiDPI: each surface at its output's scale, fractional too (`wp-fractional-scale`, `wp-viewporter`) | 🟡 done by iced_exwlshell, never tried on a scaled output: a scenario at scale 2 and 1.5 | yes |
| a scale of the whole UI (`[general] ui_scale`, over the output's) | 🔲 (iced's `scale_factor`, unused) | yes |
| a surface's size before layout | 🟡 measured by hand, duplicating the layout: the source of most workarounds | yes |

## 10. Tools

| Feature | State | Needed? |
|---|---|---|
| `debug widgets/surfaces/cursor` with the theme's selectors | ✅ | |
| showcase, headless scenarios | ✅ | |
| warnings on theme errors | ✅ | |

## 11. Structure

| Feature | State | Needed? |
|---|---|---|
| the toolkit as its own crate | 🔲 | yes |
| base stylesheet split: the toolkit's (its widgets' defaults) and Aria's (the shell's elements), loaded in that order; a theme stays one file, styling both in one cascade | 🔲 | yes |
| the toolkit's gallery, in a plain iced window (no layer-shell) | 🔲 | yes |
| performance measurements (layout and draw times, CPU of an idle bar) | 🔲 | yes |
