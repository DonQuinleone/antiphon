# Antiphon brand guidelines

The identity was agreed before any code was written. An antiphon
is sung in call and response; the brand carries that quietly, as
a modern geometric identity with the liturgical thread present
only as an undercurrent.

## The mark

A podatus: the two-note chant figure in which a low note is
answered by a higher one. Drawn geometrically as a vertical
slate stem, a solid gold square low on its left, and an open
gold-outlined square high on its right.

Variants, all in assets/:

- logo.svg: for dark backgrounds
- logo-light.svg: for light backgrounds
- tile.svg: square icon (self-contained background; safe on any
  surface)

At 16 px the stem is dropped and the two notes alone carry the
mark. Do not recolour, rotate, or add effects.

## Palette (Vespers)

| Role                     | Hex     |
| ------------------------ | ------- |
| Depth (darkest ground)   | #0c0f1d |
| Surface                  | #141a2e |
| Raised surface, borders  | #1e2742 |
| Lines, muted structure   | #2c3860 |
| Slate (secondary text)   | #7a86ad |
| Parchment (primary text) | #e9e4d4 |
| Gold (accent)            | #d9ad52 |
| Light gold (highlight)   | #edd08a |
| Gold on light grounds    | #b8862f |
| Light ground             | #f5f1e6 |
| Rubric (removals, error) | #c4576a |
| Sage (additions, ok)     | #8ba7a3 |

## Wordmark

Lowercase "antiphon" in a geometric sans (production asset to be
set in an open-licensed face and converted to outline paths).
Letter-spacing slightly open. The i is drawn dotless with a gold
diamond as its dot, centred on the stem (subtract any tracking
before centring) and raised above x-height. Tagline, always
plain: "A modern mail client for the terminal."

Pending assets: outlined-path wordmark and README banner
(blocked on choosing and fetching an open-licensed geometric
face), and the demo GIF.

## In the product

- Default theme is the house theme, derived from this palette.
- Unread marks are small gold diamonds.
- Diffs render additions in sage and removals in rubric.
- No other glyph play: the versicle mark as a prompt was
  considered and rejected for terminal font-support reasons.

## Demo GIF storyboard (~35 s loop)

1. Launch and vault unlock (Touch ID); inbox paints instantly
2. Unified inbox across six accounts
3. Rapid folder and account switching, no spinners
4. Scoped search over 300k messages, scope chips narrowing
5. Reading a mailing-list patch, coloured diff, reply-to-list
6. Compose in embedded Neovim, autocomplete, signing toggled on
7. Send, and a desktop notification arriving meanwhile
8. Close card: mark, wordmark, tagline, repository URL
