# Diskwell website

A static landing page for Diskwell: `index.html`, `styles.css`, `demo.js`, `icon.svg` (a copy of `app/IconSource/roomy-icon.svg`) and `og.jpg` (the share preview image). No build step and no dependencies; fonts come from Google Fonts.

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
- `og:image` in `index.html` points at `og.jpg` with a relative path. Social sites need an absolute URL, so change it to `https://<domain>/og.jpg` once the domain is picked.
