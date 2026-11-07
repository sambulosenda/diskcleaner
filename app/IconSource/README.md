# Roomy app icon

`roomy-icon.svg` is the source for the app icon (the "lifting off the drive" design: blocks rising out of a drive, for space being freed).
It is drawn on Apple's 1024 × 1024 macOS icon grid: the rounded square is
824 × 824 with a 100 px margin, which leaves room for the drop shadow.

The PNGs in `app/Roomy/Assets.xcassets/AppIcon.appiconset/` are rendered
from it at 16, 32, 64, 128, 256, 512 and 1024 px. After editing the SVG,
regenerate them, for example with librsvg (`brew install librsvg`):

```sh
cd app/Roomy/Assets.xcassets/AppIcon.appiconset
for px in 16 32 64 128 256 512 1024; do
  rsvg-convert -w "$px" -h "$px" ../../../IconSource/roomy-icon.svg -o "icon_$px.png"
done
```

This folder is not part of the Xcode project and is not shipped in the app.
