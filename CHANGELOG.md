# Changelog

## Unreleased

### Features/Changes
- [#2723](https://github.com/lapce/lapce/pull/2723): Line wrapping based on width (no column-based yet)
- [#1277](https://github.com/lapce/lapce/pull/1277): Error message prompted on missing git user.email and/or user.name
- [#2910](https://github.com/lapce/lapce/pull/2910): Files can be compared in the diff editor

### Bug Fixes
- [#2779](https://github.com/lapce/lapce/pull/2779): Fix files detection on fresh git/VCS repository

## 0.3.1

### Features/Changes

### Bug Fixes
- [#2754](https://github.com/lapce/lapce/pull/2754): Don't mark nonexistent files as read only (fix saving new files)
- [#2819](https://github.com/lapce/lapce/issues/2819): `Save Without Formatting` doesn't save the file

## 0.3.0

### Features/Changes
- [#2190](https://github.com/lapce/lapce/pull/2190): Rewrite with Floem UI
- [#2425](https://github.com/lapce/lapce/pull/2425): Reimplement completion lens
- [#2498](https://github.com/lapce/lapce/pull/2498): Show Lapce as an option when doing "Open With..." on Linux
- [#2549](https://github.com/lapce/lapce/pull/2549): Implement multi-line vim-motion yank and delete (`3dd`, `2yy`, etc.)
- [#2553](https://github.com/lapce/lapce/pull/2553): Implement search and replace
- [#1809](https://github.com/lapce/lapce/pull/1809): Implement debug adapter protocol

### Bug Fixes

- [#2650](https://github.com/lapce/lapce/pull/2650): Inform language servers that Lapce supports LSP diagnostics

## 0.2.8

### Features/Changes

- [#1964](https://github.com/lapce/lapce/pull/1964): Add option to open files at line/column
- [#2403](https://github.com/lapce/lapce/pull/2403): Add basic Vim marks feature

### Bug Fixes

## 0.2.7

### Features/Changes

### Bug Fixes
- [#2209](https://github.com/lapce/lapce/pull/2209): Fix macOS crashes
- [#2228](https://github.com/lapce/lapce/pull/2228): Fix `.desktop` entry to properly associate with Lapce on Wayland

## 0.2.6

### Breaking changes

- [#1820](https://github.com/lapce/lapce/pull/1820): Add remote svg icon colour to theme, disable plugin settings when none are available
- [#1988](https://github.com/lapce/lapce/pull/1987): Replace modal status background with background/foreground theme keys

### Features/Changes
- [#1899](https://github.com/lapce/lapce/pull/1899): Improve sorting files with numbers
- [#1831](https://github.com/lapce/lapce/pull/1831): Plugin settings shown on right click
- [#1830](https://github.com/lapce/lapce/pull/1830): Adds Clojure language support
- [#1835](https://github.com/lapce/lapce/pull/1835): Add mouse keybinds
- [#1856](https://github.com/lapce/lapce/pull/1856): Highlight git/VCS modified files in explorer, palette, and buffer tabs
- [#1574](https://github.com/lapce/lapce/pull/1574): Panel sections can be expanded/collapsed
- [#1938](https://github.com/lapce/lapce/pull/1938): Use dropdown for theme selection in settings
- [#1960](https://github.com/lapce/lapce/pull/1960): Add sticky headers and code lens for PHP
- [#1968](https://github.com/lapce/lapce/pull/1968): Completion lens (disabled by default)
  - ![image](https://user-images.githubusercontent.com/13157904/211959283-c3229cfc-28d7-4676-a50d-aec7d47cde9f.png)
- [#1972](https://github.com/lapce/lapce/pull/1972): Add file duplication option in fs tree context menu
- [#1991](https://github.com/lapce/lapce/pull/1991): Implement rendering of images in markdown views
- [#2004](https://github.com/lapce/lapce/pull/2004): Add ToggleHistory command
- [#2033](https://github.com/lapce/lapce/pull/2033): Add setting for double click delay (Currently only works for opening file from the explorer)
- [#2045](https://github.com/lapce/lapce/pull/2045): Add 'Rename Symbol' option on right-click
- [#2071](https://github.com/lapce/lapce/pull/2071): Add command and keybinds to delete line
- [#2073](https://github.com/lapce/lapce/pull/2073): Add Ctrl+{a,e,k} keybinds on macOS
- [#2128](https://github.com/lapce/lapce/pull/2128): Add Lapce app icon to logo collection
- [#2127](https://github.com/lapce/lapce/pull/2127): Extended double-click options with file-only and file + folders mode
- [#1944](https://github.com/lapce/lapce/pull/1944): Add filter input in git branch selection
  - ![image](https://user-images.githubusercontent.com/4404609/211232461-293e3b31-4e17-457e-825c-3018699a6fc2.png)

### Bug Fixes
- [#1911](https://github.com/lapce/lapce/pull/1911): Fix movement on selections with left/right arrow keys
- [#1939](https://github.com/lapce/lapce/pull/1939): Fix saving/editing newly saved-as files
- [#1971](https://github.com/lapce/lapce/pull/1971): Fix up/down movement on first/last line
- [#2036](https://github.com/lapce/lapce/pull/2036): Fix movement on selections with up/down arrow keys
- [#2056](https://github.com/lapce/lapce/pull/2056): Fix default directory of remote session file picker
- [#2072](https://github.com/lapce/lapce/pull/2072): Fix connection issues from Windows to lapce proxy
- [#2069](https://github.com/lapce/lapce/pull/2045): Fix not finding git repositories in parent path
- [#2131](https://github.com/lapce/lapce/pull/2131): Fix overwriting symlink
- [#2188](https://github.com/lapce/lapce/pull/2188): Fix auto closing matching pairs in inappropriate inputs

## 0.2.5

### Breaking changes

- [#1726](https://github.com/lapce/lapce/pull/1726): Add more panel theme keys, apply hover first, then current item colour

### Features/Changes
- [#1791](https://github.com/lapce/lapce/pull/1791): Add highlighting for scope lines
- [#1767](https://github.com/lapce/lapce/pull/1767): Added CMake tree-sitter syntax highlighting
- [#1759](https://github.com/lapce/lapce/pull/1759): Update C tree-sitter and highlight queries
- [#1758](https://github.com/lapce/lapce/pull/1758): Replaced dlang syntax highlighting
- [#1713](https://github.com/lapce/lapce/pull/1713): Add protobuf syntax and highlighting
- [#1720](https://github.com/lapce/lapce/pull/1720): Display signature/parameter information from LSP
- [#1723](https://github.com/lapce/lapce/pull/1723): In the palette, display the keybind for a command adjacent to it
- [#1722](https://github.com/lapce/lapce/pull/1722): Add 'Save without Formatting'; Add option to disable formatting on autosave
- [#1741](https://github.com/lapce/lapce/pull/1741): Add syntax highlighting for glsl
- [#1756](https://github.com/lapce/lapce/pull/1756): Add support for ssh port with ```[user@]host[:port]```
- [#1760](https://github.com/lapce/lapce/pull/1760): Add vim motions `cw`, `ce`, `cc`, `S`, and QOL modal bind `gf`
- [#1770](https://github.com/lapce/lapce/pull/1770): Add support for terminal tabs

### Bug Fixes
- [#1771](https://github.com/lapce/lapce/pull/1771): Update tree-sitter-bash
- [#1737](https://github.com/lapce/lapce/pull/1726): Fix an issue that plugins can't be upgraded
- [#1724](https://github.com/lapce/lapce/pull/1724): Files and hidden folders no longer will be considered when trying to open a plugin base folder
- [#1753](https://github.com/lapce/lapce/pull/1753): Limit proxy search response size in order to avoid issues with absurdly long lines
- [#1805](https://github.com/lapce/lapce/pull/1805): Fix some LSP bugs causing code actions to not show up correctly

## 0.2.4

### Features/Changes
- [#1700](https://github.com/lapce/lapce/pull/1700): Add prisma syntax and highlighting
- [#1702](https://github.com/lapce/lapce/pull/1702): Improved svelte treesitter queries
- [#1690](https://github.com/lapce/lapce/pull/1690): Add codelens and sticky headers for Dart
- [#1711](https://github.com/lapce/lapce/pull/1711): Add zstd support for plugin
- [#1715](https://github.com/lapce/lapce/pull/1715): Add support for requests from plugins

### Bug Fixes
- [#1710](https://github.com/lapce/lapce/pull/1710): Fix autosave trying to save scratch files
- [#1709](https://github.com/lapce/lapce/pull/1709): Fix search result ordering
- [#1708](https://github.com/lapce/lapce/pull/1708): Fix visual issue when search panel is placed to either side panel

## 0.2.3

### Features/Changes

- [#1655](https://github.com/lapce/lapce/pull/1655): Add status foreground theme key, use author colour for plugin version
- [#1646](https://github.com/lapce/lapce/pull/1646): Fork the process when started from terminal
- [#1653](https://github.com/lapce/lapce/pull/1653): Paint plugin icons
- [#1644](https://github.com/lapce/lapce/pull/1644): Added "Reveal in File Tree" action to the editor tabs context menu
- [#1645](https://github.com/lapce/lapce/pull/1645): Add plugin search

### Bug Fixes

- [#1651](https://github.com/lapce/lapce/pull/1651): Fixed an issue where new windows would never be created after closing all windows on macOS.
- [#1637](https://github.com/lapce/lapce/issues/1637): Fix python using 7 spaces instead of 4 spaces to indent
- [#1669](https://github.com/lapce/lapce/issues/1669): It will now remember panel states when open a new project
- [#1706](https://github.com/lapce/lapce/issues/1706): Fix the issue that color can't be changed in theme settings

## 0.2.2

### Features/Changes

- [#1643](https://github.com/lapce/lapce/pull/1643): Use https://plugins.lapce.dev/ as the plugin registry
- [#1620](https://github.com/lapce/lapce/pull/1620): Added "Show Hover" keybinding that will trigger the hover at the cursor location
- [#1619](https://github.com/lapce/lapce/pull/1619):
  - Add active/inactive tab colours
  - Add primary button colour
  - Add hover effect in source control panel
  - Add colour preview in settings
- [#1617](https://github.com/lapce/lapce/pull/1617): Fixed a stack overflow that would crash lapce when attempting to sort a large number of PaletteItems
- [#1609](https://github.com/lapce/lapce/pull/1609): Add syntax highlighting for erlang
- [#1590](https://github.com/lapce/lapce/pull/1590): Added ability to open file and file diff from source control context menu
- [#1570](https://github.com/lapce/lapce/pull/1570): Added a basic tab context menu with common close actions
- [#1560](https://github.com/lapce/lapce/pull/1560): Added ability to copy active editor remote file path to clipboard
- [#1510](https://github.com/lapce/lapce/pull/1510): Added support to discard changes to a file
- [#1459](https://github.com/lapce/lapce/pull/1459): Implement icon theme system
  - **This is a breaking change for colour themes!**
  - Colour themes should now use `[color-theme]` table format in theme TOML
  - `volt.toml` now use `color-themes` and `icon-themes` keys. `themes` key is not used anymore.
- [#1554](https://github.com/lapce/lapce/pull/1554): Added XML language support
- [#1472](https://github.com/lapce/lapce/pull/1472): Added SQL language support
- [#1531](https://github.com/lapce/lapce/pull/1531): Improved Ctrl+Left command on spaces at the beginning of a line
- [#1491](https://github.com/lapce/lapce/pull/1491): Added Vim shift+c to delete remainder of line
- [#1508](https://github.com/lapce/lapce/pull/1508): Show in progress when Lapce is self updating
- [#1475](https://github.com/lapce/lapce/pull/1475): Add editor setting: "Cursor Surrounding Lines" which sets minimum number of lines above and below cursor
- [#1525](https://github.com/lapce/lapce/pull/1525): Add editor indent guide
- [#1521](https://github.com/lapce/lapce/pull/1521): Show unique paths to disambiguate same file names
- [#1452](https://github.com/lapce/lapce/pull/1452): Wrap selected text with brackets/quotes
- [#1421](https://github.com/lapce/lapce/pull/1421): Add matching bracket highlighting
- [#1541](https://github.com/lapce/lapce/pull/1541): Order palette items according to last execute time

### Bug Fixes

- [#1566](https://github.com/lapce/lapce/pull/1565)|[#1568](https://github.com/lapce/lapce/pull/1568): Use separate colour for drag and drop background
- [#1459](https://github.com/lapce/lapce/pull/1459): Fix opening currently used logfile
- [#1505](https://github.com/lapce/lapce/pull/1505): Fix proxy download for hosts with curl without -Z flag
- [#1483](https://github.com/lapce/lapce/pull/1483): Fix showing the close icon for the first tab when opening multiple tab
- [#1477](https://github.com/lapce/lapce/pull/1477): Now use `esc` to close searchbar regarless of the current focus
- [#1507](https://github.com/lapce/lapce/pull/1507): Fixed a crash when scratch buffer is closed
- [#1547](https://github.com/lapce/lapce/pull/1547): Fix infinite cycle in workspace symbol search
- [#1628](https://github.com/lapce/lapce/pull/1541): Fix kts files not being recognized

## 0.2.1

### Features/Changes

- [#1050](https://github.com/lapce/lapce/pull/1050): Collapse groups of problems in the problem list panel
- [#1165](https://github.com/lapce/lapce/pull/1165): Command to reveal item in system file explorer
- [#1196](https://github.com/lapce/lapce/pull/1196): Always show close button on focused editor tabs
- [#1208](https://github.com/lapce/lapce/pull/1208): Sticky header breadcrumbs
  - This provides a header at the top which tells you information about the current scope! Especially useful for long blocks of code
  - ![image](https://user-images.githubusercontent.com/13157904/195404556-2c329ebb-f721-4d55-aa22-56a54f8e8454.png)
  - As well, you can see that there is now a breadcrumb path to the current file.
  - A language with syntax highlighting can have this added, even without an LSP. Take a look at `language.rs` if your language isn't supported!
- [#1198](https://github.com/lapce/lapce/pull/1198): Focus current theme/language in palette
- [#1244](https://github.com/lapce/lapce/pull/1244); Prettier plugin panel
- [#1238](https://github.com/lapce/lapce/pull/1238): Improved multicursor selection
- [#1291](https://github.com/lapce/lapce/pull/1291): Use link colour for empty editor buttons
- [#1234](https://github.com/lapce/lapce/commit/07390f0c90c0700d1f69409bf48723d15090c474): Automatic line height
- [#1262](https://github.com/lapce/lapce/pull/1262): Add absolute/relative copy path to file explorer
- [#1284](https://github.com/lapce/lapce/pull/1284): Render whitespace (default: none)
  - ![image](https://user-images.githubusercontent.com/13157904/195410868-f27db85f-d7d2-4197-84f0-12d6c44e2053.png)
- [#1308](https://github.com/lapce/lapce/pull/1308): Handle LSP ResourceOp
- [#1251](https://github.com/lapce/lapce/pull/1251): Add vim's paste-before `P` command
- [#1319](https://github.com/lapce/lapce/pull/1319): Add information page for plugins
- [#1344](https://github.com/lapce/lapce/pull/1344): Replace the branch-selector menu with a scrollable list
- [#1352](https://github.com/lapce/lapce/pull/1352): Add duplicate line up/down commands
- [#1281](https://github.com/lapce/lapce/pull/1281): Implement logic for displaying plugin installation status
- [#1353](https://github.com/lapce/lapce/pull/1353): Implement syntax aware selection
- [#1358](https://github.com/lapce/lapce/pull/1358): Add autosave implementation
- [#1381](https://github.com/lapce/lapce/pull/1381): Show multiple hover items in the hover box
- [#1040](https://github.com/lapce/lapce/pull/1040): Add keybindings for `Shift-Del`, `Shift-Ins`, and `Ctrl-Ins`
- [#1401](https://github.com/lapce/lapce/pull/1401): Merge semantic and tree-sitter syntax highlighting
- [#1426](https://github.com/lapce/lapce/pull/1426): Add cursor position/current selection in status bar
  - ![image](https://user-images.githubusercontent.com/13157904/195414557-dbf6cff1-3ab2-49ec-ba9d-c7507b2fc83a.png)
- [#1420](https://github.com/lapce/lapce/pull/1420): Add LSP `codeAction/resolve` support
- [#1440](https://github.com/lapce/lapce/pull/1440): IME support
- [#1449](https://github.com/lapce/lapce/pull/1449): Plugin settings in the editor support. Though this still needs some work from plugins to expose them all nicely!
- [#1441](https://github.com/lapce/lapce/pull/1441): Button for Case-Sensitive search
- [#1471](https://github.com/lapce/lapce/pull/1471): Add command to (un)install Lapce from/to PATH
- [#1419](https://github.com/lapce/lapce/pull/1419): Add atomic soft tabs: now you can move your cursor over four spaces as if it was a single block

### Syntax / Extensions

- [#957](https://github.com/lapce/lapce/pull/957): Replace existing tree-sitter syntax highlighting code with part of Helix's better implementation
  - This means that syntax highlighting for more languages! Such as fixing markdown support, and making so that languages embedded in others (like JavaScript in HTML) work.
  - Note that not all themes have updated themselves to include the extra scopes/colors.
- [#1036](https://github.com/lapce/lapce/pull/1036): Recognize ESM/CJS extensions for JavaScript/TypeScript
- [#1007](https://github.com/lapce/lapce/pull/1007): Add ability to bind a key shortcut for quitting the editor
- [#1104](https://github.com/lapce/lapce/pull/1104): Add syntax highlighting for Dockerfile, C#, and Nix
- [#1118](https://github.com/lapce/lapce/pull/1118): Recognize `pyi, pyc, pyd, pyw` extensions for Python
- [#1122](https://github.com/lapce/lapce/pull/1122): Recognize extensions for DLang
- [#1335](https://github.com/lapce/lapce/pull/1335): Highlighting for DLang
- [#1153](https://github.com/lapce/lapce/pull/1050): Recognize and add highlighting for Dart
- [#1161](https://github.com/lapce/lapce/pull/1161): Recognize and add highlighting for Svelte and LaTeX files
- [#1299](https://github.com/lapce/lapce/pull/1299): Recognize and add highlighting for Kotlin
- [#1326](https://github.com/lapce/lapce/pull/1326): Recognize and add highlighting for Vue
- [#1370](https://github.com/lapce/lapce/pull/1370): Recognize and add highlighting for R
- [#1416](https://github.com/lapce/lapce/pull/1416): Recognize and add highlighting for Scheme
- [#1145](https://github.com/lapce/lapce/pull/1145): Adds/Fixes highlighting for C/C++/TypeScript/JavaScript/Zig/Bash
- [#1272](https://github.com/lapce/lapce/pull/1272): Adds/Fixes highlighting for Elm/JSX/TSX
- [#1450](https://github.com/lapce/lapce/pull/1450): Add `tf` extension for HCL

### Bug Fixes

- [#1030](https://github.com/lapce/lapce/pull/1030): Don't try to open an font file with an empty name if there is no font family set
- [9f0120d](https://github.com/lapce/lapce/commit/9f0120df85e3aaaef7fbb43385bb15d88443260a): Fix excessive CPU usage in part of the code
- [bf5a98a](https://github.com/lapce/lapce/commit/bf5a98a6d432f9d2abdc1737da2d075e204771fb): Fix issue where sometimes Lapce can't open
- [#1084](https://github.com/lapce/lapce/pull/1084): Use host shell in terminal when running inside Flatpak
- [#1120](https://github.com/lapce/lapce/pull/1120): Make Alt+Backspace work in the terminal properly
- [#1127](https://github.com/lapce/lapce/pull/1127): Improve Julia highlighting
- [#1179](https://github.com/lapce/lapce/pull/1179): Various improvements/fixes to window-tab functionality
- [#1210](https://github.com/lapce/lapce/pull/1210): Fixed closing modified file when closing split
- [#1219](https://github.com/lapce/lapce/pull/1219): Fix append command behavior
- [#1250](https://github.com/lapce/lapce/pull/1250): Fix too long socket path for proxy
- [#1252](https://github.com/lapce/lapce/pull/1252): Check whether the active editor tab index actually exists, avoiding a potential crash
- [#1294](https://github.com/lapce/lapce/pull/1294): Backward word deletion should respect whitespace better
- [#1301](https://github.com/lapce/lapce/pull/1301): Fix incorrect path when going from Url -> PathBuf (such as from an LSP)
- [#1368](https://github.com/lapce/lapce/pull/1368): Fix tabstop for postfix completions
- [#1388](https://github.com/lapce/lapce/pull/1388): Fix regex search within the terminal
- [#1423](https://github.com/lapce/lapce/pull/1423): Fix multiple cursor offset after inserting opening pair
- [#1434](https://github.com/lapce/lapce/pull/1434): Join PATH with correct platform separator
- [#1443](https://github.com/lapce/lapce/pull/1443): Correct terminal font sizing
- [#1453](https://github.com/lapce/lapce/pull/1453): Trim whitespace from search results
- [#1461](https://github.com/lapce/lapce/pull/1461): Load shell environment when launching from GUI
- Many more fixes!

### Other

- [#1191](https://github.com/lapce/lapce/pull/1191): Tone down default inlay hint background color in Lapce dark theme
- [#1227](https://github.com/lapce/lapce/pull/1227): Don't restore cursor mode on undo
- [#1413](https://github.com/lapce/lapce/pull/1413): Disable format-on-save by default. Remember to re-enable this if you want it!
- [#1404](https://github.com/lapce/lapce/pull/1404): Log panics with full backtrace as error
