# Contributing to hardcalc

Thanks for your interest! Bug reports, ideas and pull requests are welcome.

## Reporting bugs and ideas

Open an [issue](https://github.com/hardcalc/hardcalc/issues). For a bug,
the most helpful things are a small `.hcmd` file that shows it, what you did,
what you expected and what happened, plus your operating system and
terminal. Security problems go through [SECURITY.md](SECURITY.md) instead.

## Building and testing

You need Rust (<https://rustup.rs>).

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo test --features typst                         # with the built-in Typst compiler
python3 -m unittest discover -s libreoffice/tests   # LibreOffice extension
```

CI runs the same on Linux, macOS and Windows.

## Pull requests

- Keep a pull request to one topic and add tests for what it changes.
- The file format is described in [SPEC.md](SPEC.md) and
  [CONFIG-SPEC.md](CONFIG-SPEC.md); a change to the format updates them, and
  the Python port in [libreoffice/](libreoffice/) follows.
- Files are written in a canonical form (`hardcalc fmt`) with numbers in
  decimal-point notation; keep examples and test data that way.
- Key bindings live in `src/tui/keymap.rs`; after a change, regenerate the
  matrix with `cargo run -- --keymap-markdown > docs/keys.md`.
- Documentation and code comments are in English.

By contributing you agree that your contribution is licensed under the
[GPL-3.0-or-later](LICENSE), like the rest of the project.
