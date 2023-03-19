# Building from source

It is easy to build Lapce from source on a Linux distribution. Cargo handles the build process, all you need to do, is ensure the correct dependencies are installed.

1. Install the Rust compiler and Cargo using [`rustup.rs`](https://rustup.rs/). If you already have the toolchain, ensure you are using version 1.64 or higher.

2. Install dependencies for your operating system:

## Ubuntu

```sh
apt install cmake pkg-config libfontconfig-dev libgtk-3-dev g++
```

## Fedora

```sh
dnf install gcc-c++ perl-FindBin perl-File-Compare gtk3-devel
```

3.Clone this repository, enter it and install lapce:

```sh
git clone https://github.com/lapce/lapce.git
cd ./lapce
cargo install --path . --bin lapce --locked
```

> If you use a different distribution, and are having trouble finding appropriate dependencies, let us know in an issue!

Once Lapce is compiled, the executable will be available in `$HOME/.cargo/bin`.
