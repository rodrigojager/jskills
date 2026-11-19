# 25-source-assets

## Purpose
Heavy source files that generate the final assets of the product.

## What goes here
- `.blend`, `.psd`, `.spine`, `.kra` and similar authoring files.

## What does NOT go here
- Final exports used by the product (the product's `assets/`).
- Mock images (`20-design/`).

## Conventions
- One source file per asset, named after the asset it produces.
- A natural candidate to live on another disk: `jproject storage move`.

## When to delete
- When the user decides.
