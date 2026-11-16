# UI style

Hearth's interface is a fire seen at night: warm charcoal panels with round
corners, one ember-amber accent, and text the colour of ash-white paper.
Controls look like controls — a slider has a track and a knob, a switch is a
pill — so nobody has to learn what `=====-----` means. Readable at arm's length
in a headset, finished enough not to look like a debug overlay, and defined in
one place so a reskin is a change of colours rather than a change of code.

**Everything lives in `client/godot/ui.gd` (`UiStyle`).** If you are about to
write a `Color(...)` in interface code, you are about to make the next reskin
harder. Use a token.

## Layout of the menu

Tap the menu button (Y on the keyboard, Y/B in the headset) and three panels
open together, hinged like a dressing-table mirror so the side ones turn in
towards you:

```
 ╭─ Friends ──── 2 online ─╮ ╭─ 🔥 Hearth ─────────── × ─╮ ╭─ Chat ──── the loft ──╮
 │ FRIENDS  2 / 5 online ── │ │ [ Worlds | Camera | Settings ] │ Bob  21:04            │
 │ (A)• Ada        [ Join ] │ │ SOUND ──────────────────── │ hey, anyone up for     │
 │      in the loft         │ │ (🎙) Microphone      (●━) │ the forest?            │
 │ (B)  Bob                 │ │ (🔈) Music   ━━━●───  60%  │ Cat  21:05             │
 │      offline             │ │ (🔈) Sound   ━━━━━●─  80%  │ on my way              │
 │ IN THIS EMBER  3 ─────── │ │ VIEW ───────────────────── │                        │
 │ (K)• Ada     you         │ │ (◐) Mirror         People ›│ ╭────────────────────╮ │
 │ (C)• Cat     here        │ │ (⇥) Leave this flame       │ │ Say something...   │ │
 ╰──────────────────────────╯ ╰────────────────────────────╯ ╰────────────────────────╯
```

* **Left, Friends** (`WING_WIDTH` wide): your friends, online first, each with
  where they are and a **Join** pill; requests with **Accept**; a field to add
  somebody; then everyone in this ember, you included.
* **Middle, the menu**: a title bar (flame, the ember you are in, an × to
  close, a handle to carry it by) and three tabs — **Worlds**, **Camera**,
  **Settings**.
* **Right, Chat**: the last `CHAT_LINES` lines said in this ember, a speaker's
  name once per run of lines, and the field you type into.

Carry the menu by its title bar and the side panels come with it.

**Hold** the menu button instead and the **quick menu** comes up by your hand:
four round buttons — Camera, Chat, Mic, Menu. Point with the other hand and
click. It is for the things you reach for in the middle of something else.

## The tokens

| Token | What it is for |
|---|---|
| `BACKDROP` | Behind everything; the inside of a field; text on an accent button. |
| `INK` | A panel. Charcoal, slightly see-through so the world stays present. |
| `INK_LIFT` | A surface on a panel: a key cap, an icon chip, an off switch, a slider track. |
| `EDGE` | The hairline round a panel, a field, a divider. Low alpha: an edge catching firelight. |
| `EDGE_BRIGHT` | The rim along a panel's top, the drag handle. |
| `ACCENT` | The live thing: selected, on, filled, the button to press. One per screen's worth of meaning. |
| `TEXT` | Anything you are meant to read. |
| `TEXT_DIM` | Headings, hints, notes, anything you cannot act on. |
| `WARN` | Refused, leaving, quitting. |
| `GOOD` | Online, done. |
| `SHADOW` | The soft pool under every panel. |

Sizes come from there too. In metres, for panels in the world: `RADIUS`,
`RADIUS_SMALL`, `PAD`, `ROW`, `HEADING`, `SPACER`, `LINE`, `ROW_TALL`,
`ROW_BUTTON`, `CHIP`, `ICON`, `TAB`, `TAB_ICON`, `TRACK`, `KNOB`, `SWITCH`,
`READOUT`, `HAIRLINE`, `SHADOW_SPREAD`, `VALUE_AT`. In pixels, for flat
controls: `BORDER_2D`, `RADIUS_2D`, `PAD_2D`, `FONT_2D`.

## One shape

Every surface — a panel, a chip, a switch, a field, a knob, the selection — is
`UiStyle.rounded()`: one quad and one tiny shader that draws a rounded
rectangle from its signed distance. That makes it exact at any size, anti-
aliased by its own screen derivative, and no dearer than the flat quad it
replaced. A border, a soft edge (the shadow) and a sheen from the top are
uniforms. `UiStyle.circle()` is the same thing with the radius at half the side.

A panel's body is `UiStyle.frame()`: the shadow, the charcoal surface with its
hairline, and a rim of light along the top edge, which is what makes a flat
panel read as a thing with thickness.

## Rows

A panel of rows is a `WorldMenu`. What a row dictionary has decides what is
drawn (the full list is at the top of `menu.gd`):

| Row has | Drawn as |
|---|---|
| `value` | a slider: track, amber fill, white knob, the percentage at the end |
| `toggle` | a switch at the right: amber with the knob right when on |
| `choice` | its current value in amber, and a chevron: picking it cycles |
| `text` | a sunken field in the value column (across the row if the label is empty) |
| `button` | a filled pill across the panel, its label centred |
| `heading` | small capitals with a hairline running to the edge |
| `icon` | the icon on a rounded chip before the label |
| `avatar` | the person's initial on their own colour, with an online dot if `status` says |
| `sub` | a second, quieter line under the label |
| `note` / `action` / `more` | quiet text, a small amber pill, or a chevron at the right end |
| `small` | a short line — chat — in `accent` for a name, `bright` for what was said |
| `picture` | a picture across the row in a rounded frame: the last photo you took |

Rows are not all one height: a heading is shorter than a row, a friend with a
second line is taller. `WorldMenu.row_y(i)` says where any row is.

## Text has a place

Text is never dropped wherever it fits. Every piece of it has a slot.

- **Inset.** `PAD` from the panel edge, both sides. Text never touches an edge.
- **Two columns.** A label goes on the left; fields, slider tracks and choices
  start at `VALUE_AT` across the panel, so they line up with each other.
- **Centred in its row,** so the selection lands *on* the text.
- **Fitted.** `UiStyle.fit()` shrinks a label a little and then cuts it short
  with an ellipsis, against the room its row leaves it — so a long name never
  runs into the switch beside it or off the panel. Chat is wrapped by
  `UiStyle.wrap()` into lines that fit before it is drawn.

## Contrast, guaranteed

Every glyph carries a dark outline, set by `UiStyle.label()`. A panel is
translucent and the world behind it is any colour at all, so text that only
works against the panel it was designed on stops working the moment somebody
walks behind it.

On top of that the tokens themselves are checked: `UiStyle.contrast()` is the
WCAG ratio, and `tests/panel_test.gd` fails the build if body text drops below
**7.0** on any surface or a meaning colour below **4.5** on a panel.

| | ratio |
|---|---|
| `TEXT` on a panel | 17.5 : 1 |
| `TEXT` on a key cap | 15.3 : 1 |
| `TEXT` on the backdrop | 18.2 : 1 |
| `ACCENT` on a panel | 9.7 : 1 |
| `GOOD` on a panel | 13.3 : 1 |
| `WARN` on a panel | 7.4 : 1 |
| `TEXT_DIM` on a panel | 6.8 : 1 |
| `TEXT` on a selected row (`SELECT_ALPHA`) | 10.3 : 1 |
| `ACCENT` on a selected row | 5.7 : 1 |
| `TEXT` on a highlighted key (`KEY_ALPHA`) | 7.5 : 1 |
| `TEXT` on a pressed button | 10.7 : 1 |
| `BACKDROP` on an accent button | 10.1 : 1 |

`UiStyle.over()` mixes in **linear** space, because that is where the GPU does
it; mixing sRGB values badly under-reads how bright a translucent overlay comes
out. For a background that is not a token — an avatar's colour, a swatch — use
`UiStyle.on(surface)`, which picks whichever of `TEXT` or `BACKDROP` reads.

## The rules

1. **No literal colours in interface code.** Tokens only. The check:

   ```
   grep -n "Color(0\." client/godot/*.gd
   ```

   The only hits should be `ui.gd` itself and things that are *content*, not
   interface: `world_format.gd` (a world's defaults), `elevator.gd` (the lift's
   metal), `mirror.gd` (its frame and the room it renders), `studio.gd` (an
   object's colour) and `photo_camera.gd` (the camera's body and lens).
2. **Interface is unshaded.** A panel that dims when you walk away from a
   brazier is a panel you cannot read.
3. **Interface wins the depth test.** Text, icons and shapes all ignore it, so
   a crate cannot eat half a row and a menu opened near a wall is still a menu.
   Draw order is `render_priority` alone (below).
4. **Build text with `_label()`** (in a panel) or `UiStyle.label()`. Never a
   bare `Label3D`: that is how a label ends up with no outline or the wrong size.
5. **Build shapes with `UiStyle.rounded()`.** Never a bare quad for a surface;
   even a hairline is a rounded shape, because a 4 mm quad seen at an angle
   aliases into a dashed line.
6. **One accent.** If two things on screen are both "the live thing", one of
   them is wrong. A filled amber button is the one thing a page is for.
7. **Icons are drawn, not typed.** An SVG from `client/godot/icons/` through
   `UiStyle.icon()`, tinted like the text beside it. One stroke weight (5), one
   64-unit grid, round caps, no fills. They are imported at **2× with mipmaps**
   (`svg/scale=2.0`, `mipmaps/generate=true` in each `.import`), so they are
   sharp up close and do not shimmer across a room; `UiStyle.icon()` measures
   the texture, so the import scale never changes their size on a panel. Set
   both on any new icon.
8. **Content keeps its own colours.** An avatar, a world, a photo, a flame: you
   are looking *at* those, not at the frame round them.

## Draw order

Interface ignores the depth buffer, so `render_priority` decides what covers
what. Within one panel: `LAYER_BODY` (shadow and surface), `LAYER_WASH` (the
selection), `LAYER_FILL` (chips, fields, switches, tracks), `LAYER_EDGE`
(knobs, markers, hairlines, outlines), `LAYER_TEXT`.

Priority is **global**, so a panel that must sit in front of another lifts its
whole range by a band: the menu, its side panels and the keyboard are
`BAND`, the quick menu `BAND * 2`, the toast `BAND * 3`. Inside a panel, build
with `_label()` and add `band` to every layer, so it lands in that panel's band.

## Seeing it

`tools/shot.gd` renders the real scene to `shots/`:

```
godot --path client/godot --script res://tools/shot.gd --resolution 1280x720
```

Not `--headless` — there has to be a renderer for there to be a picture. It
writes the lift panel, typing, the key grid, the menu with both side panels on
each tab, the quick menu, the camera in hand, and the advanced pages. Every
contrast and layering bug in this file was found by looking at those.

## The two halves

**Panels in the world** (the lift panel, the menu and its side panels, the
quick menu, the keyboard) are built from `UiStyle` directly and extend
`FlatPanel`, which gives them the hit test and `_frame`.

**Flat controls** (the launcher, the world builder) are styled by
`client/godot/ui_theme.tres`, which `project.godot` applies to every Control.
It is **generated** from the same tokens by `tools/make_theme.gd`, so it cannot
drift from them; `client/build.ps1` regenerates it on every build.

## Reskinning

Change the colours at the top of `ui.gd` and build. The panels read the tokens
at runtime and the theme is regenerated from them. Changing the *shape* — the
radius, the shadow, the rim — is `UiStyle.frame()`, `ROUNDED_SHADER` and
`UiStyle._box()`, still one file.
