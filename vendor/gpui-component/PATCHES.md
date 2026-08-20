# Local patches

This directory vendors `gpui-component 0.5.1` from crates.io so Corbit can
carry one focused upstream fix without moving to the unreleased main branch.

- `6903579a817cacad3078005f1458e75a4f3291b9` — preserve directional Markdown
  selection endpoints, fixing malformed multi-line selections when dragging
  upward or back across the anchor line.

The patch can be removed when a published `gpui-component` release contains
that commit and Corbit has upgraded to it.
