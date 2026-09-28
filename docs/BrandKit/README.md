# CommitBook brand assets

The approved primary identity is green on pure black: a bright green front page, darker green history pages, pale mint text and checked dot, on a pure black background. The pale-green-background icon remains secondary. All three dots have equal radius. The timeline starts below the front sheet; lower dots align with their paper bottoms. App framing uses the approved 18% enlargement.

**Use the named assets below.** [Preview all final assets](index.html).

| Asset | Use |
|---|---|
| `commitbook-logo.svg` | Transparent, editable logo master with semantic color tokens |
| `commitbook-logo.png` | Transparent 512 × 512 raster logo |
| `commitbook-app-icon.svg` | 1024 × 1024 opaque square vector source; no baked corner mask |
| `commitbook-app-icon.png` | 1024 × 1024 opaque raster app-icon source |
| `commitbook-product-icon.svg` | Resolution-independent website/product icon, opaque square background |
| `commitbook-product-icon.png` | 512 × 512 website fallback |
| `commitbook-github-avatar.svg` | Editable square GitHub avatar source |
| `commitbook-github-avatar.png` | 512 × 512 opaque avatar, ready for the CommitBook organization |
| `commitbook-favicon.png` | 32 × 32 raster favicon derivative |
| `commitbook-app-icon-secondary.svg` / `.png` | 1024 × 1024 pale-green secondary icon |

## Usage and color customization

The SVG logo keeps the approved geometry and these editable root CSS variables:

- `--current`, `--middle`, `--oldest`: front, middle, and back paper colors. History nodes use their matching paper tokens.
- `--rail`: pale timeline line.
- `--text`, `--dot`, `--check`: text strokes, checked-dot fill, and checkmark.

Primary colors: front `#3fb950`, middle `#318447`, oldest `#245936`, rail `#78a883`, text and checked dot `#e3f2e6`, check `#3fb950`, background `#000000`. The secondary retains its previous dark-front/light-history green palette on `#e9f3eb`. To recolor, edit the root variables or use inline SVG; CSS in a host document does not override an SVG loaded through `<img>`. Product/app SVGs resolve colors to literal values for broad tool compatibility.

The PNGs are rendered directly from their matching SVGs. Keep the transparent logo and opaque square icon distinct. Do not add a rounded mask to the app-icon source: native platforms apply their own. Native Icon Composer appearance/material validation remains a separate packaging step.

## Product page

The linked ZAAI.com site uses `public/commitbook/commitbook-product-icon.svg`. The hero and product content metadata reference it, so product cards and navigation consumers use the same artwork. The assets in this repository now use the approved green-on-black identity. The external site copy has not been updated in this update; copy the named product SVG, PNG, and favicon there when syncing the site, including its `icon.png` compatibility alias.

## GitHub organization

[Preview the new GitHub avatar](github-avatar-preview.html) in square and circular crops, plus small UI sizes. The 512px PNG uses the approved green-on-black identity and is ready to upload.

Upload `commitbook-github-avatar.png` in **CommitBook → Settings → Upload new picture**. The composition is checked in both square and circular crops. Creating this file does not change the live organization avatar.

References: [GitHub organization profile instructions](https://docs.github.com/en/organizations/collaborating-with-groups-in-organizations/customizing-your-organizations-profile), [image requirements](https://docs.github.com/en/account-and-profile/reference/profile-reference).

## Retained assets

Only the approved green-on-black identity, pale-green secondary, and their required logo, product, favicon, and GitHub avatar exports are retained. Superseded variants, archives, and comparison pages have been removed.
