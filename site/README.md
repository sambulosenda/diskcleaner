# Roomy website

A static landing page for Roomy: `index.html`, `styles.css`, `demo.js` and `icon.svg` (a copy of `app/IconSource/roomy-icon.svg`). No build step and no dependencies; fonts come from Google Fonts.

The demo in the page runs on made-up example items in `demo.js`. It never reads the visitor's Mac.

## Preview locally
```sh
cd site
python3 -m http.server 8000
# open http://localhost:8000
```

## Still to do
- The download card (`#download` in `index.html`) is a placeholder until pricing and licensing are decided.
- Hosting and domain are not chosen yet. Any static host works (GitHub Pages, Netlify, Cloudflare Pages): publish the `site/` folder as it is.
