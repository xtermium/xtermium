# Contributing to xtermium

Thanks for helping. Bug reports, fixes and features are all welcome. For anything large, open an
issue first so we can agree on the approach before you write it.

## Branches and pull requests

- Nobody pushes to `main`, maintainers included. Every change reaches it as a pull request from a
  `feat/<topic>` or `fix/<topic>` branch, and the branch is merged with a merge commit. Squash and
  rebase merges are off, and the merged branch is deleted.
- Start the branch from an up-to-date `main`. Outside contributors: fork, branch in your fork, and
  open the pull request against `xtermium/xtermium`'s `main`.
- One topic per pull request. Keep the diff to the change: don't reformat or reorder code you
  didn't touch.
- Run the checks before you open it. A plain `cargo` command needs the RDP helper built once:
  run `npm run rdp` first.

  ```sh
  npm run check && npm test
  cargo clippy --all-targets && cargo test
  (cd rdp && cargo clippy --all-targets)   # when rdp/ changed
  ```

## Commits and release notes

Each commit has a short summary line, then a body of bullets saying what changed. The pull
request's title and those bullets become the release notes (`scripts/changelog.mjs`), so write
them for a developer reading what changed.

## Versions

Leave the version alone (`Cargo.toml`, `package.json`). A merge that raises it publishes a
release (`.github/workflows/release.yml`), and maintainers do that.

## Conventions

`CLAUDE.md` holds the project's conventions; it's written for coding agents, but they apply to
everyone. `DESIGN.md` covers the UI. `npm test` enforces some of them, such as the spacing scale
and the locale files.

## Security problems

Don't open a public issue. Report them privately: **Security › Report a vulnerability** on the
repository.

## Licence

Your contribution is licensed under the project's licence, the Apache License 2.0 (`LICENSE`).
"xtermium" and its logo are trademarks of the author; forks must use another name.
