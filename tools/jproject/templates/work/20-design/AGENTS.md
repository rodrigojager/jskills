# 20-design

## Purpose
Visual reference material: what the product should look like and what it looks like now.

## What goes here
- `mocks-img/`: mock images of what is wanted.
- `mock-html/`: HTML mocks built on top of the mock images.
- `previews/`: screenshots/previews of the real result; only the latest of each.

## What does NOT go here
- Final assets used by the product (the product's `assets/`).
- Heavy source files such as `.psd` or `.blend` (`25-source-assets/`).

## Conventions
- One canonical file per artifact with a fixed name (e.g. `inventory-screen.png`), edited in place.
- Alternatives go in `_variants/<artifact>/a.png`, `b.png`... next to the canonical file.
- Only the user consolidates: the chosen variant replaces the canonical file and `_variants/<artifact>/` is deleted at once.

## When to delete
- Variants: on consolidation. Old previews: when a new one is generated.
