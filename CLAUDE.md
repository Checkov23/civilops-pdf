# CivilOps PDF: instructions for agents

CivilOps PDF is CivilOps's PDF program for plans and documents, written in Rust (egui), running
locally on Windows. `AGENTS.md` holds the binding rules (assets, clean-room, automation, never
crash); this file holds the day-to-day working instructions. If the two disagree, `AGENTS.md` wins.

## Start here
- `README.md`: what the program does, how to build and release it, and where things are.
- The README of the crate you touch (`crates/<name>/README.md`).
- `parity/acrobat-features.toml`: the feature checklist (`cargo xtask parity`). Mark a feature
  `shipped` only with a passing test as evidence, and say in its notes what is still missing.

## Non-negotiables
- **Assets:** read `AGENTS.md` §1 before adding or showing any icon, image, font or document.
  Every asset has an entry in `ATTRIBUTION.toml`; `cargo xtask assets` enforces it.
- **Clean-room:** never copy GPL/AGPL code; behaviour comes from public specifications
  (ISO 32000-2, the Arlington model) and black-box observation (`AGENTS.md` §2).
- **Privacy:** synthetic fixtures only. Never commit personal data or customer documents.
- **Layering:** nothing below L7 depends on egui/winit/eframe/rfd (`cargo xtask layers`).
  `cos`/`filters`/`crypt`/`arlington` stay standalone.
- **Fidelity:** the PDF object graph is the model. Preserve unknown data. Saves are incremental
  unless a full rewrite is required. Never silently drop or repair data without recording it.
- **Never panic** (`AGENTS.md` §4): `Result` and `?`; no `unwrap`/`expect`/`panic!` outside tests
  unless provably infallible (with a comment); no `unsafe` (`unsafe_code = "forbid"`); `get()`
  instead of indexing with input-derived positions; checked arithmetic; bounded recursion; handle
  lock poisoning. Every crash fix gets a synthetic regression test.
- **Rust only** in the product and its build (`xtask`); Python only for the release scripts in
  `packaging/civilops/`.
- **Language:** text the user sees is written in English in the code and translated in
  `crates/ui-egui/src/i18n_de.rs` (German is the default). Keep keys and translations in step; a
  test rejects duplicates.
- **Quality gates** before every commit: `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `cargo xtask layers`, `cargo xtask wasm`, `cargo xtask assets`, `cargo xtask parity`.
- **Commits:** author `Oliver Schiemann <228913879+Checkov23@users.noreply.github.com>`, German
  messages, no attribution lines for tools or agents. Commit only green states.
- **Line endings:** the repository stores bytes as they are (`* -text`). Write LF; on Windows,
  Python's `write_text` writes CRLF, so write bytes.

## Running and looking at the app
- `cargo run -p printcraft -- <file.pdf>` opens the program (`civilops-pdf.exe`).
- `cargo run -p printcraft-cli -- run --script steps.json --root DIR` drives the engine headlessly
  through the automation tools (`printcraft-cli tools` lists them). Use it, with `page_render`, to
  check engine changes.
- For UI work, look at the result. Start the app with `--control ctl.json <file.pdf>`, then use
  `printcraft-cli ui --control ctl.json screenshot --out x.png`, `inspect query=…`,
  `click label=…` or `click x= y= [count=2]`, `drag from=[x,y] to=[x,y]`,
  `key key=A modifiers=["command"]`, `type text="…"` (quoted: values are read as JSON),
  `command id=…` and `state`. Screenshot pixels / 2 = logical pixels; take page positions from
  `ui state` (`pages_on_screen`). Control-channel tests use kittest
  (`crates/ui-egui/tests/control.rs`).
- `cargo xtask fuzz --time 300` mutation-fuzzes open/render/edit/save in child processes; turn
  every real finding into a small synthetic regression test before fixing it.
- Parallel agents: use a separate `CARGO_TARGET_DIR` per agent and separate git worktrees.

## Releases
- `python packaging/civilops/release.py --version X --notizen-datei n.md --hochladen` builds,
  writes `Drittanbieter-Lizenzen.txt` (`packaging/civilops/drittanbieter.py`), packs with Velopack
  and uploads to the protected test channel. Upload target and credentials live outside the
  repository (`../civilops-pdf-releases/server.txt`, `zugang-test.txt`).
- Test-channel packages carry that channel's credentials: never attach them to a public GitHub
  release. Public releases carry builds of the public channel only.
