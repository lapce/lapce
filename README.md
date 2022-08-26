<h1 align="center">
  <a href="https://lapce.dev" target="_blank">
  <img src="extra/images/logo.png" width=200 height=200/><br>
  Lapce
  </a>
</h1>

<h4 align="center">Lightning-fast And Powerful Code Editor</h4>

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


Lapce ([IPA: /læps/](https://en.wiktionary.org/wiki/lapse)) is written in pure Rust with a UI in [Druid](https://github.com/linebender/druid) (which is also written in Rust). It is designed with [Rope Science](https://xi-editor.io/docs/rope_science_00.html) from the [Xi-Editor](https://github.com/xi-editor/xi-editor) which makes for lightning-fast computation, and leverages [OpenGL](https://www.opengl.org/) for rendering. More information about the features of Lapce can be found on the [main website](https://lapce.dev) and user documentation can be found on [GitBook](https://docs.lapce.dev/).

![](https://github.com/lapce/lapce/blob/master/extra/images/screenshot.png?raw=true)

## Features

* Built-in LSP ([Language Server Protocol](https://microsoft.github.io/language-server-protocol/)) support to give you intelligent code features such as: completion, diagnostics and code actions
* Modal editing support as first class citizen (Vim-like, and toggleable)
* Built-in remote development support inspired by [VSCode Remote Development](https://code.visualstudio.com/docs/remote/remote-overview). Enjoy the benefits of a "local" experience, and seamlessly gain the full power of a remote system.
* Plugins can be written in programming languages that can compile to the [WASI](https://wasi.dev/) format (C, Rust, [AssemblyScript](https://www.assemblyscript.org/))
* Built-in terminal, so you can execute commands in your workspace, without leaving Lapce.

## Contributing

Guidelines for contributing to Lapce can be found in [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Installation

### Arch Linux

There is an community package that can be installed with `pacman`:

```bash
sudo pacman -Syu lapce
```

### Fedora
```bash
sudo dnf copr enable titaniumtown/lapce
sudo dnf install lapce
```

### Flatpak

Lapce is available as a flatpak [here](https://flathub.org/apps/details/dev.lapce.lapce)

```bash
flatpak install flathub dev.lapce.lapce
```

### Homebrew

```bash
brew install lapce
```

### Scoop

```bash
scoop install lapce
```

### winget

You can find the packages [here](https://github.com/microsoft/winget-pkgs/tree/master/manifests/l/Lapce/Lapce):

```bash
winget install lapce
```

## Building from source

It is easy to build Lapce from source on a GNU/Linux distribution. Cargo handles the build process, all you need to do, is ensure the correct dependencies are installed.

1. Install the Rust compiler and Cargo using [`rustup.rs`](https://rustup.rs/). If you already have the toolchain, ensure you are using version 1.62 or higher.

2. Install dependencies for your operating system:

#### Ubuntu
```sh
sudo apt install cmake pkg-config libfontconfig-dev libgtk-3-dev
```
#### Fedora
```sh
sudo dnf install gcc-c++ perl-FindBin perl-File-Compare gtk3-devel
```

3. Clone this repository (this command will clone to your home directory):
```sh
git clone https://github.com/lapce/lapce.git ~/lapce
```

4. `cd` into the repository, and run the build command with the release flag
```sh
cd ~/lapce
```

```sh
cargo build --release
```

> If you use a different distribution, and are having trouble finding appropriate dependencies, let us know in an issue!

Once Lapce is compiled, the executable will be available in `target/release/lapce`.

## Feedback & Contact

The most popular place for Lapce developers and users is on the [Discord server](https://discord.gg/n8tGJ6Rn6D).

Or, join the discussion on [Reddit](https://www.reddit.com/r/lapce/) where we are just getting started.

There is also a [Matrix Space](https://matrix.to/#/#lapce-editor:matrix.org), which is linked to the content from the Discord server.

## License

Lapce is released under the Apache License Version 2, which is an open source license. You may contribute to this project, or use the code as you please as long as you adhere to its conditions. You can find a copy of the license text within `LICENSE`.
