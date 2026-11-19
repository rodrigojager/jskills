# plan-to-html

Convert one or more Markdown files into an HTML reader with navigation, search, images, diagrams, and light and dark modes. The Python renderer preserves the source and applies layout and styling. An agent can add visual enhancements where interpretation helps.

A self-contained output embeds CSS, JavaScript, images, and fonts. Open it locally with `file://` and read it offline.

## Installation

Install the skill with the jskills installer. From the installed `plan-to-html` directory, run the setup command below.

```sh
python scripts/setup.py --skip-mermaid
```

Setup requires Python 3.10 or later and `pip`. It installs pinned packages in `.vendor/` without changing your global Python environment. The skill includes Mermaid, local fonts, and their licenses. The `--skip-mermaid` flag uses the bundled copy. Open a new agent session if the skill has yet to appear. The entry point is [SKILL.md](SKILL.md).

## Usage

Ask your agent to use `plan-to-html` and point it at a Markdown file or plan directory. In Codex, you can invoke it with `$plan-to-html`.

Run these commands from the skill directory. Input and output paths can point to your own workspace.

```sh
# Generate a self-contained file.
python scripts/plan_html.py build path/to/PLAN.md --output output/plan.html --theme papel --format single

# Render the bundled example.
python scripts/plan_html.py build examples/reading-plan --output output/example.html --theme neon-laser --format single
```

To edit CSS, JavaScript, or visual assets before bundling, build an editable version first.

```sh
python scripts/plan_html.py scan path/to/PLAN.md --output output/outline.json
python scripts/plan_html.py build path/to/PLAN.md --output work/plan.html --theme manuscrito
python scripts/bundle_html.py work/plan.html --output output/plan.html
```

The editable HTML keeps resources as separate files. Bundling embeds local SVG, PNG, BMP, fonts, CSS, and JavaScript. Missing or external static resources stop bundling by default. Ordinary links to external websites remain links.

For extra JavaScript module imports, run `python scripts/setup.py --with-js-bundler` with Node.js and npm available. Standard rendering uses Python.

## Features

- Navigation across documents and sections, search, and direct links to passages.
- Light and dark modes, reading preferences, saved reading position, responsive layout, and printing.
- Syntax highlighting, code copying, line wrapping, and sticky table headers.
- Zoomable images, Mermaid diagrams, and optional visuals specified in JSON.
- A map of links between documents. Selecting a document shows its neighbors. Connections route around cards, and dense maps reveal links on demand.
- Deterministic Markdown conversion that preserves the document body.

## Themes and preview

Open the self-contained [preview.html](preview.html) from your checkout to explore 20 themes in light and dark modes. For generated readers, choose the theme before rendering.

Included themes are `papel`, `grafite`, `carbono`, `jade`, `prisma`, `caderno`, `coral`, `studio`, `iris`, `polar`, `neon-laser`, `manuscrito`, `crayon`, `papiro`, `jornal`, `moderno`, `minimalista`, `maximalista`, `art-deco`, and `terminal`. You can also use `default` and `slate`.

Put a compatible CSS file in `themes/` and select it with `--theme NAME`, or pass the path directly. The [theme contract](references/themes.md) describes the variables and conventions for creating a theme.

## References

| File | Contents |
| --- | --- |
| [SKILL.md](SKILL.md) | Agent instructions and editorial rules |
| [Content visuals](references/content-visuals.md) | When to add charts, diagrams, timelines, and other visuals |
| [Enhancements](references/enhancements.md) | Visual enhancement data format |
| [Themes](references/themes.md) | Variables, organization, and theme contract |
| [Reading map](references/reading-map.md) | Map interaction and edge routing |
| [Packaging](references/packaging.md) | Offline packaging and additional assets |
| [Theme preview example](examples/theme-preview/README.md) | Reference research, generation, and preview tests |

## Verification

```sh
python scripts/check_renderer.py
```

For browser tests, install Playwright in your development environment and run `python -m playwright install chromium`. Packaging tests with module imports also require setup with `--with-js-bundler`.

```sh
python scripts/check_bundle.py
python scripts/check_reader_tools.py
python scripts/check_reading_map.py
python scripts/check_browser.py
```

The checks cover source preservation, embedded assets, reader controls, the reading map, color modes, narrow screens, and offline use. They write to temporary directories or `.test-output/`.

## Dependencies and visual references

Python dependencies are pinned in [requirements.txt](requirements.txt). Mermaid and the bundled fonts carry their licenses under `assets/vendor/` and `assets/fonts/`.

The theme research used [design-inspiration-mcp](https://github.com/notsointresting/design-inspiration-mcp). Research notes and the [reference license](examples/theme-preview/DESIGN-INSPIRATION-MCP-LICENSE.txt) are included with the preview example. The MCP is optional for repeating the research. Generating and reading HTML do not require it.
