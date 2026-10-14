# Yozora brand

| File | Size | Use |
|---|---|---|
| `banner.png` | 1280×400 at 2x | Top of the README |
| `social.png` | 1280×640 at 2x | GitHub social preview and link cards |
| `poster.png` | 1200×1600 at 2x | Posters and announcements |
| `mark.png` | 512×512 at 2x, transparent | Avatar, favicon |

The mark is a Y traced as a constellation, for 夜空, "night sky". The app icon is
`packaging/icons/yozora.svg`, and the 16 px glyph is `Icon::Logo` in `crates/yozora-ui/src/icons.rs`.
Everything uses the Midnight tokens and the Starlight accent from `crates/yozora-ui/src/theme.rs`.

The images are HTML pages in `src/`. To change one, edit it and run `./render.sh`, which needs
Google Chrome.
