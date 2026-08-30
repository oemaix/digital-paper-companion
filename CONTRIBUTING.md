# Contributing

Bug reports and pull requests are welcome. For a large change, open an issue
first so we can agree on the approach.

## Setup

How to build and run the app is in the [README](README.md#development).
The product definition lives in [`docs/`](docs/).

## Pull requests

- Keep the change focused; do not mix unrelated refactors.
- Run `npm run check` before you push (frontend typecheck/lint plus Rust
  fmt, clippy and tests).
- Match the style of the surrounding code.
- If you change behaviour that is specified in `docs/`, update the matching
  document in the same PR.

## License

By contributing you agree that your work is dual-licensed under
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at the recipient's
option — the same terms as the rest of the project.
