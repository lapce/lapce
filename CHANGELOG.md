# Changelog

## Unreleased

### Added

- Added Dockerfile, C# & Nix tree sitter hightlight [#1104]
- Added DLang LSP langue ids [#1122]
- Implemented logic for getting the installation progress [#1281]
- Render whitespace [#1284]
- Implemented syntax aware selection [#1353]
- Added autosave configuration [#1358]
- IME support for macOS [#1440]

### Fixed

- Fixed high CPU issue when editor font family is empy [#1030]
- Fixed an issue that sometimes Lapce can't open [bf5a98a6d432f9d2abdc1737da2d075e204771fb]
- Much improved tree sitter highlight [#957]
- Fixed terminal issues under flatpak [#1135]
- Fixed auto-completion crash [#1366]
- Fixed hover hints + show multiple hover hint items [#1381]