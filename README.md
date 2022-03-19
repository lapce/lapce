<h1 align="center">
  <a href="https://lapce.dev" target="_blank">
  <img src="extra/images/logo.png" width=200 height=200/><br>
  Lapce
  </a>
</h1>

<h4 align="center">Lightning-fast and Powerful Code Editor written in Rust</h4>

<div align="center">
  <a href="https://github.com/lapce/lapce/actions/workflows/cargo.yml" target="_blank">
    <img src="https://github.com/lapce/lapce/actions/workflows/cargo.yml/badge.svg" />
  </a>
  <a href="https://discord.gg/n8tGJ6Rn6D" target="_blank">
    <img src="https://img.shields.io/discord/946858761413328946?logo=discord" />
  </a>
  <a href="https://matrix.to/#/#lapce-editor:matrix.org" target="_blank">
    <img src="https://img.shields.io/matrix/lapce-editor:matrix.org?color=turquoise&logo=Matrix" />
  </a>
  <a href="https://docs.lapce.dev" target="_blank">
      <img src="https://img.shields.io/static/v1?label=Docs&message=docs.lapce.dev&color=blue" alt="Lapce Docs">
  </a>
</div>
<br/>


Lapce is written in pure Rust, with UI in [Druid](https://github.com/linebender/druid). It's using [Xi-Editor](https://github.com/xi-editor/xi-editor)'s [Rope Science](https://xi-editor.io/docs/rope_science_00.html) for text editing and [Wgpu](https://github.com/gfx-rs/wgpu) for rendering. More information on the [website](https://lapce.dev).

![](https://github.com/lapce/lapce/blob/master/extra/images/screenshot.png?raw=true)

## Features

* Modal Editing (Vim-like) support as a first-class citizen (can be turned off as well)
* Built-in LSP support
* Built-in remote development support (inspired by [VSCode Remote Development](https://code.visualstudio.com/docs/remote/remote-overview))
* Plugin can be written in programming languages that can compile to [WASI](https://wasi.dev/) (C, Rust, [AssemblyScript](https://www.assemblyscript.org/))
* Built-in terminal

## Build from source

### Install the Rust compiler with `rustup`

1. Install [`rustup.rs`](https://rustup.rs/).

### Dependencies
#### Ubuntu
```sh
sudo apt-get install cmake pkg-config libfreetype6-dev libfontconfig1-dev libxcb-xfixes0-dev libxkbcommon-dev
```
### Building
```sh
cargo build --release
```
The exectuable will be available at `target/release/lapce`

## Feedback

* Chat on [Discord](https://discord.gg/n8tGJ6Rn6D)
* Join the discussion on [Reddit](https://www.reddit.com/r/lapce/)
