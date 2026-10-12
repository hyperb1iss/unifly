# Circuit Dragonfly artwork

The production assets live in [`docs/static/brand`](../docs/static/brand).
The SVG files are the editable masters. Every file is self-contained, with
outlined lettering and no external fonts, images, scripts, or filters.

The identity uses the flat Circuit Dragonfly approved by Bliss: purple upper
wings, cyan lower wings, and a pale central spine. The mark was redrawn as
vector paths from an imagegen concept. The dimensional glass exploration is
not part of the identity or the distributed artwork.

The public [brand page](https://hyperb1iss.github.io/unifly/brand/) lists downloads, colors,
and usage guidance. Keep the shared wing geometry consistent when editing
the standalone marks, lockups, banners, and icons.

## Exports

PNG files are convenience exports of the matching SVG masters. Regenerate
them from the repository root with `resvg` available on your PATH:

```sh
./artwork/export.sh
```

The exports use resvg 0.48.1. The repository card is 1280×640 pixels; the
avatar is 512×512; the browser icon is 32×32; the touch icon is 180×180.
The standalone mark PNG is 1280×720, and horizontal logo PNGs are 1120×320.
Logo and mark PNGs preserve transparency.

The README banner is 1600×480. The docs hero is 1536×1024. Both remain SVG
so their flat shapes stay sharp at any display density.

## Typography and source

Lettering uses Inter 4.1 (Regular, Medium, and Bold), converted to paths
with `usvg`. No font installation is needed to display or export the assets.
Inter is available from [its upstream project](https://github.com/rsms/inter),
under the [SIL Open Font License](Inter-OFL.txt).

The initial concept prompt described an angular four-wing dragonfly with
purple upper wings, cyan lower wings, and an off-white central spine on a
dark violet background. The final artwork is authored SVG; it contains no
generated raster imagery.

The PNG repository card can be uploaded through GitHub's repository
settings under **Social preview**. Committing the file does not change that
repository setting. The docs template references the card automatically
for Open Graph and Twitter previews after deployment.
