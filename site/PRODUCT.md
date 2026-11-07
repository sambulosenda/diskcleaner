# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(The website is web. The product it sells is a native macOS app, macOS 13 Ventura or later, Apple silicon and Intel, distributed outside the Mac App Store.)

## Users

Primary: everyday Mac users who have seen "Storage almost full" and cannot tell what is safe to delete. Typically a 256 GB MacBook holding iPhone backups and years of Downloads and Messages attachments. They want a clear answer to "what can I delete?" with no jargon and no risk. The site speaks to them first.

Secondary, in this order:
- Developers (Xcode, Android, Docker, Node, Python, Rust, local AI models), served by Developer mode. This is the second pitch, not the lead.
- Creators with large render and media caches.
- Students and family Mac users on shared or older Macs.
- People who look after several Macs for family or clients.

## Product Purpose

Shows what is filling a Mac's disk, explains every item in plain words, and frees space safely. Success means the user understands their storage after one scan, gets meaningful space back without losing anything they needed, and trusts the app enough to pay.

## Positioning

The cleaner that explains itself and refuses to scare:
- Every item says what it is, why it exists and whether it comes back.
- Every item carries one of three labels: Likely safe, Review first or Keep. Personal content is never Likely safe.
- Nothing is ticked for the user. Items go to the Trash first, Put back undoes the last clean-up, and every path is checked again just before acting.
- No automatic clean-ups, no inflated "junk" totals, no AI deciding what is safe, and file names never leave the Mac.
- It explains what is inside "System Data", the grey bar macOS leaves unexplained.
- Developer mode covers every toolchain in one notarised app and shows the commands it runs.

Against consumer cleaners it wins on honesty. Against disk visualisers it wins on meaning, not just size. Against single-ecosystem dev tools it wins on breadth.

## Operating Context

Visitors are usually evaluating a utility after a low-storage warning. They need to believe it will not delete something they care about. The site has a hands-on demo on a made-up example Mac (`demo.js`) that never reads the visitor's Mac.

## Capabilities and Constraints

- Free scan, plus up to 5 GB of clean-up in total; a paid licence unlocks unlimited clean-up and the advanced analysers.
- **Undecided:**
  - Pricing and the download link. The download card is a placeholder.
  - Hosting and domain.
  - Launch date. The product is "in testing".
- The site is static HTML/CSS/JS with no build step. Fonts come from Google Fonts.
- Terms used by the product: Likely safe, Review first, Keep, Put back, Developer mode, System Data.

## Brand Commitments

- **Name:** Diskwell (official, confirmed by Sam on 7 Nov 2026). It replaces the codename "Roomy". The app code, bundle ID and repo still say Roomy until a separate rename task. Don't build identity on puns of the old name.
- **Voice:**
  - Plain British English, short sentences.
  - No jargon in simple-mode copy.
  - No alarming words ("junk", "danger", "infected").
  - Calm, not salesy.

## Evidence on Hand

- The example-Mac demo data in `demo.js`.
- The app icon (`icon.svg`, copied from `app/IconSource/roomy-icon.svg`).
- The share image `og.jpg`.
- The PRD at `../docs/PRD.md`.

There are no real user numbers, beta results, testimonials, customer logos or press. None of these may be invented. Example figures must be clearly labelled as coming from the example Mac.

## Product Principles

1. Explain before acting: every claim or item is made understandable in one plain sentence.
2. Trust over totals: never inflate, frighten or rush; when unsure, do less.
3. The user decides: nothing is pre-ticked or automatic, and every action can be undone.
4. Everyday first, depth on request: the simple story leads, and developer detail is there for those who want it.
5. Show, do not claim: the demo proves the behaviour instead of adjectives.

## Accessibility & Inclusion

The audience includes non-technical and older users on shared Macs. Copy must read plainly, and the interactive demo must work by keyboard and with reduced motion. Body text should meet at least WCAG AA contrast; the current page has known contrast failures.
