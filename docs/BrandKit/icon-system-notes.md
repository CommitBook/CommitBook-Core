# CommitBook connected icon system

Open `icon-system-exploration.html` directly in a browser. It contains 12 new directions (50–61), palette controls, app-icon and bare-mark views, light/dark/tint studies, a single-color test, and SVG downloads. Existing explorations and the original combined icon are unchanged.

## Chosen direction

The user selected **111 / White dot**. `icon-111-refinements.html` now compares **12 options, 112–123**, all with a white checked node, smaller borderless dots, and the line starting only at the front sheet’s bottom edge (y=62, flat cap). Horizontal paper steps stay equal at 14 units; vertical steps increase to 12, 13, or 14 units. Four dot-size pairs are compared per spacing. The lower edges of dots two and three align with the bottom edges of their respective papers; their centers account for each variant’s dot radius. The user chose **113 / Balanced small · 12 step**. Current sources are `commitbook-113-logo.svg` and `commitbook-113-app-square.svg`. Paper steps are 14 units right and 12 down; the checked node radius is 4.5 and lower-node radii are 3.5. The preview defaults and resets to 113. Earlier exports remain historical explorations.

## Previous A/B comparison

`icon-dot-treatments.html` compares **110 / A — Dark violet dot** and **111 / B — White dot**. All papers use the same 14-unit horizontal and 11-unit vertical step, correcting the uneven 14/8 horizontal offsets of 103. All three timeline circles are borderless and vertically aligned. A uses the chosen color darkened 35% toward black with a white check; B uses a white dot with a check in the chosen color. Sources: `commitbook-110-logo.svg`, `commitbook-110-app-square.svg`, `commitbook-111-logo.svg`, and `commitbook-111-app-square.svg`. The user subsequently selected B / 111.

## Prior positioning studies

`icon-vertical-timeline-studies.html` contains **12 positioning variants, 98–109**, correcting the stepped timeline: all three node centers share one x-coordinate in every variant. The studies vary rear-paper rightward offsets, the timeline inset inside the front corner, and vertical spacing. Four text lines and the no-tail timeline remain. **103 / Balanced inset** is a tentative recommendation, not a user-approved choice. Sources: `commitbook-103-logo.svg` and `commitbook-103-app-square.svg`.

## Prior experiment (stepped timeline, superseded)

**97 / Integrated timeline** is in `icon-integrated-timeline.html`. It removes the fifth text line, moves the checked node into the front sheet’s bottom-left corner, and runs a stepped timeline downward to nodes on the left edges of sheets two and three. The composition is recentered after removing the separate left rail. Editable sources: `commitbook-97-logo.svg` and `commitbook-97-app-square.svg`. This is a new experiment; 96 remains intact for comparison.

## Previous composition

The latest composition is **96 / Five lines, refined** in `icon-top-left-stack.html`. The front sheet is upper left and the oldest sheet is lower right. The timeline remains on the left and ends at dot three, with no tail. Its visible bounds align with the paper stack: top y=13, bottom y=84. Node centers are y=20, 49.85, and 79.7, with equal center-to-center spacing. The five text strokes begin near the top and use lengths **13, 25, 25, 20, 25** units: short, long, long, slightly shorter, long. All have equal weight and spacing, with no heading hierarchy.

Current editable sources are `commitbook-96-logo.svg` and `commitbook-96-app-square.svg`. The no-fold geometry, smaller check, lighter rail, and customizable matched page/node colors remain. Earlier sources, including 95, are exploration history rather than the current direction.

## Previous recommendation (superseded)

**53 / Matched bridges** was the previous recommendation and was rejected by the user. The current, middle, and oldest page have matching timeline-node colors and physical connections. Unlike 50 (color only), the mapping survives monochrome. Compare 61 for heavier small-scale geometry and 56 for outlined history.

The bridges enter the exposed lower-left region of each sheet, so the timeline is lower than in concept 49. This is deliberate: direct lines can reach all three sheets without an ambiguous route through the front page. Variant 52 preserves more of the original timeline height through bent routes. Variant 59 tests links entering each exposed header instead.

At 16 px, fine check/text details are impressionistic. The full composition is intended for app-icon and branding sizes; a tiny UI glyph should receive a dedicated optical simplification after direction selection.

## Color contract

The recommended `commitbook-connected-logo.svg` exposes CSS custom properties on its root:

- `--current`: chosen base color; latest page, first node, first bridge.
- `--middle`: base mixed 30% toward white; middle page/node/bridge.
- `--oldest`: base mixed 57% toward white; oldest page/node/bridge.
- `--fold`: base mixed 73% toward white.
- `--rail`: base mixed 36% toward white.
- `--ink`, `--surface`: appearance-dependent knockout/text and background tones.

The HTML palette function derives these values from one color. Exported logo SVGs retain editable tokens and fallback colors. Update the root style to recolor a standalone SVG; a host document cannot override CSS inside an SVG loaded through `<img>`. For in-app live theming, use inline SVG or map the semantic roles to native fills.

Single-color mode makes every page/node the same foreground color and introduces narrow background-colored boundaries to distinguish sheets. Its palette is a silhouette test, not an exact simulation of Apple's system tint treatment. Light, dark, and tint selections are independent browser studies; extreme custom colors still need visual contrast review.

## iOS preparation

`commitbook-connected-app-square.svg` is an opaque 1024 × 1024 square vector source with resolved color values for design-tool import. The SVG viewBox is 128 × 128; the 96-unit mark is inset by 16 units. The preview's CSS rounded mask is not baked into the export.

Named `data-layer` groups separate the three sheets, fold, text, and timeline. These are editable grouping hints, not a native Icon Composer document. The timeline can be split into further objects during native composition.

Next steps after selecting a direction:

1. Import the chosen vector groups into Icon Composer; validate placement using Apple's template rather than treating this preview's padding as an official safe-area specification.
2. Build and inspect native default, dark, and tinted appearances on device. Avoid baking speculative Liquid Glass highlights into the geometry.
3. Bundle a curated set of color variants for Home Screen customization. Arbitrary in-app accent colors and the app's installed Home Screen icon are separate features.
4. Check Home Screen and Settings sizes before replacing any production asset. No app assets or native configuration were changed here.

References checked during this exploration:

- [Apple app-icon guidance](https://developer.apple.com/design/human-interface-guidelines/app-icons)
- [Creating an icon with Icon Composer](https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer)
- [Configuring alternate app icons](https://developer.apple.com/documentation/xcode/configuring-your-app-to-use-alternate-app-icons)
