# CommitBook brand assets

This is the final approved logo: violet stacked papers, four white text lines, a white checked dot, and two matching history dots. All three dots have equal radius. The timeline starts below the front sheet; lower dots align with their paper bottoms. App framing uses the approved 18% enlargement.

**Use the named files below, not the numbered exploration files.** [Preview all final assets](index.html).

| Asset | Use |
|---|---|
| `commitbook-logo.svg` | Transparent, editable logo master with semantic color tokens |
| `commitbook-logo.png` | Transparent 512 × 512 raster logo |
| `commitbook-app-icon.svg` | 1024 × 1024 opaque square vector source; no baked corner mask |
| `commitbook-app-icon.png` | 1024 × 1024 opaque raster app-icon source |
| `commitbook-product-icon.svg` | Resolution-independent website/product icon, opaque square background |
| `commitbook-product-icon.png` | 512 × 512 website fallback |
| `commitbook-github-avatar.png` | 512 × 512 opaque avatar, ready for the CommitBook organization |
| `commitbook-favicon.png` | 32 × 32 raster favicon derivative |

## Usage and color customization

The SVG logo keeps the approved geometry and these editable root CSS variables:

- `--current`, `--middle`, `--oldest`: front, middle, and back paper colors. History nodes use their matching paper tokens.
- `--rail`: pale timeline line.
- `--text`, `--dot`, `--check`: text strokes, checked-dot fill, and checkmark.

Default violet is `#7952c6`; middle and oldest tones are `#a186d7` and `#c5b5e6`; the rail is `#d7cbee`. Text and the checked dot are white. To recolor, edit the root variables or use inline SVG; CSS in a host document does not override an SVG loaded through `<img>`. Product/app SVGs resolve colors to literal values for broad tool compatibility.

The PNGs are rendered directly from their matching SVGs. Keep the transparent logo and opaque square icon distinct. Do not add a rounded mask to the app-icon source: native platforms apply their own. Native Icon Composer appearance/material validation remains a separate packaging step.

## Product page

The linked ZAAI.com site uses `public/commitbook/commitbook-product-icon.svg`. The hero and product content metadata reference it, so product cards and navigation consumers use the same artwork. Matching PNG and favicon derivatives are copied alongside it. `public/commitbook/icon.png` is updated as a compatibility alias, not an old logo.

## GitHub organization

Upload `commitbook-github-avatar.png` in **CommitBook → Settings → Upload new picture**. The composition is checked in both square and circular crops. Creating this file does not change the live organization avatar.

References: [GitHub organization profile instructions](https://docs.github.com/en/organizations/collaborating-with-groups-in-organizations/customizing-your-organizations-profile), [image requirements](https://docs.github.com/en/account-and-profile/reference/profile-reference).

## History

Numbered SVGs and the `icon-*.html` comparison pages are retained for provenance. The former orange icon is archived at `archive/commitbook-legacy-icon.png`. The first exploration links to that archive intentionally. Final assets must not depend on exploration filenames.
