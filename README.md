# Papillon

Sticky notes floating on your desktop, in English and French. A small
replacement for Windows' Sticky Notes, written in Rust with
[egui](https://github.com/emilk/egui).

- Each note is its own small window: move it, resize it, and it comes back
  where you left it.
- Saved as you type, on your computer only.
- Seven colors, three text sizes, dark and light themes for the list.
- A note can stay above the other windows.
- A list to find, reopen and delete notes. Closing a note keeps it in the
  list; an empty one disappears; deleting a written one asks first.
- Can start with the computer.
- A single portable executable on Windows, a Flatpak on Linux.

## Using it

The strip at the top of a note moves it, its borders resize it. Its buttons
appear when the pointer is over it or when it has the focus.

| Action | Keys |
| --- | --- |
| New note | Ctrl+N (in a note or the list) |
| Close the note | Ctrl+W |
| Undo, redo | Ctrl+Z, Ctrl+Shift+Z |
| Move the note | Alt+arrows |
| Resize the note | Alt+Shift+arrows |
| Leave the text, to reach the buttons | Escape, then Tab |

Tab in a note's text inserts a tab. In the list, Delete deletes the card that
has the keyboard focus (asking first if the note is written); search ignores
accents.

The list stays hidden while notes are on the desktop. To show it: the list
button of a note, or launch Papillon again. It shows by itself when no note is
left on the desktop; closing it then quits. Its `…` menu holds the settings and
**Quit**; there, *Bring all notes back on screen* rescues notes left on a
monitor that is gone.

If the notes file holds anything Papillon doesn't understand, it first keeps a
copy beside it (`notes.broken-….json`), shows what it could still read, and says
so in the list; the next save then replaces the file. If the file can't be read
at all, nothing is written over it: restart Papillon when it's available again.
The previous version is kept in `notes.json.bak` at the first write of each
session. The list also says when saving fails, and quitting warns first.

Notes are in `%APPDATA%\Papillon\notes.json` on Windows and in
`~/.var/app/io.github.Shhu.Papillon/config/papillon/notes.json` with the
Flatpak.

On Linux, Papillon runs through X11 (XWayland under a Wayland desktop): Wayland
doesn't let an app choose its window's position or stay on top.

Writing systems the embedded fonts lack (Chinese, Japanese, Korean, Hebrew,
Arabic, Indian scripts, Thai, Ethiopic, Tibetan, emoji…) use the system's fonts,
when installed.
Emoji show in one color. Arabic letters aren't joined and right-to-left text
runs left to right: the interface toolkit doesn't do text shaping.

On a computer with a battery and two graphics cards, Papillon uses the
economical one; `WGPU_POWER_PREF=high` or `low` overrides that choice.

## Building

Papillon needs a recent stable Rust toolchain (edition 2024, Rust 1.95 or
later).

```sh
cargo run --release
cargo test
```

Packaging scripts:

- Windows, from WSL: `packaging/windows/build.sh` (a single `papillon.exe`,
  with the C runtime linked in).
- Linux: `packaging/flatpak/build.sh` (a Flatpak bundle, built offline from
  source).

## Contributing

- Code, comments and commit messages in English.
- Every text shown to users goes through `t!` (`src/i18n.rs`), with its key in
  both `locales/en.ftl` and `locales/fr.ftl` (same keys, same order); the
  tests check that every key is translated and used.
- After any change to `Cargo.lock`: `packaging/third-party-licenses.py` (it
  fails if a crate has no license text: add it to `packaging/licenses/<crate>/`),
  and regenerate `packaging/flatpak/cargo-sources.json`.
- `cargo fmt`, `cargo clippy --all-targets` and `cargo test` must pass. The
  ignored `capture` test renders the website's picture (options at the end of
  `src/app.rs`).

## License

Papillon is licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE)
or the [MIT license](LICENSE-MIT), at your option. Unless you explicitly state
otherwise, any contribution intentionally submitted for inclusion in Papillon
shall be dual licensed as above, without any additional terms or conditions.

Third-party works embedded in the binaries are listed in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md), and their license texts in
[assets/licenses.txt](assets/licenses.txt), also under *Licenses* in the app's
menu.
