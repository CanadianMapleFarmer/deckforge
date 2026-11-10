# Contributing to deckforge

Thanks for helping out! Bug reports, controller test results and pull requests are all welcome.

## Reporting bugs

Open an [issue](https://github.com/kloppers-labs/deckforge/issues/new/choose) with your OS, deckforge version, audio device and controller. For controller problems, the MIDI log from **Settings → MIDI diagnostics** helps a lot.

## Development setup

```bash
cd apps/desktop
pnpm install
pnpm tauri dev
```

See the [README](README.md#build-from-source) for system requirements.

## Before opening a pull request

```bash
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
cd apps/desktop && pnpm check && pnpm test
```

- Keep pull requests focused on one change and describe what you tested (and on which hardware).
- Code should read as self-explanatory; prefer clear names over comments, and keep comments to one line for non-obvious constraints.
- Nothing may run on the real-time audio thread that allocates, locks or blocks.
- Match the surrounding formatting; please don't reformat files you aren't otherwise changing.

## Clean-room rule

deckforge is an independent implementation. Do not contribute code, assets, data or mapping files copied from rekordbox, Serato, Traktor, Mixxx or any other DJ software. Behaviour may be matched by ear, by measurement or from public documentation.

## License

By contributing you agree that your work is dual-licensed under MIT and Apache-2.0, like the rest of the project.
