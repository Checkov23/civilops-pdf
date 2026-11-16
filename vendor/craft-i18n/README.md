# craft-i18n (vendored catalogs)

The interface catalogs of CivilOps PDF: `locales/<code>/common.tsv` (shared by the Craft apps) and
`locales/<code>/pdfcraft.tsv` (the app's own), for the languages the interface offers
(`crates/ui-egui/src/i18n.rs`, `LANGUAGES`). `crates/ui-egui/build.rs` embeds them; `CRAFT_I18N_DIR`
may point at a craft-i18n checkout instead.

- Source: https://github.com/storytold/craft-i18n at 575cfdef63227b14daf173b8cc87928bfdfa325e
- Licence: MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`)
- Changes by CivilOps PDF: the product name `PdfCraft` is `CivilOps PDF` in both columns (the app's
  English says CivilOps PDF), the models variable is `PRINTCRAFT_MODELS` (as in the code), and `pdfcraft.tsv` ends with CivilOps PDF's own rows (its commands and
  tools: CivilOps website, Donate, Edit objects, Measure length, Set scale…). German wording of
  CivilOps PDF (formal "Sie", "Kennwort") comes from `crates/ui-egui/src/i18n_de.rs`, before these.
