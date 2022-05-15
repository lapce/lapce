<h1 align="center">
  <a href="https://lapce.dev" target="_blank">
  <img src="extra/images/logo.png" width=200 height=200/><br>
  Lapce
  </a>
</h1>

<h4 align="center">Lightning-fast and Powerful Code Editor written in Rust</h4>

<div align="center">
  <a href="https://github.com/lapce/lapce/actions/workflows/ci.yml" target="_blank">
    <img src="https://github.com/lapce/lapce/actions/workflows/ci.yml/badge.svg" />
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


Lapce is written in pure Rust, with the UI in [Druid](https://github.com/linebender/druid). It uses [Xi-Editor's](https://github.com/xi-editor/xi-editor) [Rope Science](https://xi-editor.io/docs/rope_science_00.html) for text editing, and the [Wgpu Graphics API](https://github.com/gfx-rs/wgpu) for rendering. More information can be found on the [website](https://lapce.dev).

![](https://github.com/lapce/lapce/blob/master/extra/images/screenshot.png?raw=true)

## Features

* Modal Editing (Vim like) support as first class citizen (can be turned off as well)
* Built-in LSP (Language Server Protocol) support to give you code intelligence like code completion, diagnostics and code actions etc.
* Built-in remote development support (inspired by [VSCode Remote Development](https://code.visualstudio.com/docs/remote/remote-overview)) for a seamless "local" experience, benefiting from the full power of the remote system.
* Plugins can be written in programming languages that can compile to the [WASI](https://wasi.dev/) format (C, Rust, [AssemblyScript](https://www.assemblyscript.org/))
* Built-in terminal, so you can execute commands in your workspace, without leaving Lapce.

## Contributing

The guidelines about contributing to Lapce can be found in
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## Build from source

### Install the Rust compiler with `rustup`

1. Install [`rustup.rs`](https://rustup.rs/).

### Dependencies
#### Ubuntu
```sh
sudo apt install cmake pkg-config libfreetype6-dev libfontconfig-dev libxcb-xfixes0-dev libxkbcommon-dev python3
```
### Building
```sh
cargo build --release
```
The compiled executable will be available at `target/release/lapce`

## Feedback

* Chat on [Discord](https://discord.gg/n8tGJ6Rn6D)
* Or join the discussion on [Reddit](https://www.reddit.com/r/lapce/)
