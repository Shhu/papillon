# Third-party notices

Papillon's binary embeds the following works, under their own licenses.

## Fonts

The interface font (assets/fonts) is Inter, by The Inter Project Authors, under
the SIL Open Font License 1.1: see assets/fonts/Inter-OFL.txt.

As fallback, the binary also embeds egui's fonts (`epaint_default_fonts`):
Ubuntu Light (Ubuntu Font Licence 1.0), Hack (MIT, with the Bitstream Vera
license), Noto Emoji and emoji-icon-font (SIL Open Font License 1.1).

The icons are Phosphor Icons (https://phosphoricons.com), under the MIT
license, through the `egui-phosphor` crate.

## Rust crates

Papillon is built with many Rust crates, each under its own license (MIT,
Apache-2.0, BSD, zlib, Unicode-3.0 and similar permissive licenses, choosing
the permissive option where a crate offers several); see Cargo.lock for the
list.
