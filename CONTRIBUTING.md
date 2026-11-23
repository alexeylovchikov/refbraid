# Contributing

Thank you for wanting to improve RefBraid.

## Licence of contributions

The project is under the [MIT licence](LICENSE). By sending a change (a merge or pull
request, a patch) you confirm that you wrote it or have the right to submit it, and that you
agree to it being distributed under the same licence.

## The clean-room rule

RefBraid is inspired by other tools but copies nothing from them. Please keep it so:

- Do not copy code, text, icons, images, fonts or other resources from any other product,
  open or closed, unless its licence allows it and you say where it comes from.
- Do not read, decompile or inspect the installation, sources or resources of any
  JetBrains IDE (or of another proprietary Git client) to work on this program. Describe the
  behaviour you want in your own words instead.
- Icons are drawn in code (`app/src/theme.rs`); the repository holds no image assets apart
  from the program's own icon and its own screenshots.
- A new dependency needs a licence that fits MIT distribution (MIT, Apache-2.0, BSD, Zlib,
  ISC and the like — no GPL, LGPL, AGPL or "source available" licences). After adding one,
  run `make notices` in `app/` so that `THIRD-PARTY-NOTICES.md` stays complete.

## Working on the code

`CLAUDE.md` (the same text is in `AGENTS.md`) describes the commands, the architecture and
the conventions; it is written for coding agents but is the best introduction for people
too. In short:

```bash
cd app
cargo build
cargo test
cargo run -- --screenshot out.png     # a picture of the window without a display
```

- Interface text is English in the code, wrapped in `tr("…")`; add the Russian text to
  `app/src/i18n_ru.rs`.
- Colours come from `app/src/theme.rs` and must work in both themes.
- Git is run through `git::run` / jobs, not through a library.
- Tests of git operations use `TestRepo` (`app/src/testrepo.rs`).
- Commit messages: `[feat] …`, `[fix] …`, `[docs] …`, `[chore] …` — one sentence.

Don't include personal data, passwords, tokens or internal addresses in code, tests or
screenshots; use made-up names and `example.com`.
