## Building from source

It is easy to build Lapce from source on a GNU/Linux distribution. Cargo handles the build process, all you need to do, is ensure the correct dependencies are installed.

1. Install the Rust compiler and Cargo using [`rustup.rs`](https://rustup.rs/). If you already have the toolchain, ensure you are using latest Rust version.

2. Install dependencies for your operating system:

#### Ubuntu
2.1  install dependencies
```sh
sudo apt install clang libxkbcommon-x11-dev pkg-config libvulkan-dev libwayland-dev xorg-dev libxcb-shape0-dev libxcb-xfixes0-dev
```
2.2  Follow instructions below to set path to lapce, where path to 
lapce is like: 
```
home/YOURNAME/lapce/target/release/lapce
```

2.3  Create a symbolic link:
- Open a terminal and run the following command to create a symbolic link from the executable to the chosen directory:
     ```bash
     ln -s home/YOURNAME/lapce/target/release/lapce ~/.local/bin/
     ```

2.4 Add the directory to PATH (if necessary):
     ```bash
     export PATH="home/YOURNAME/lapce/target/release/lapce:$PATH"
     ```

2.5  Refresh the shell:
- Run the following command to reload the shell configuration:
     ```
     source ~/.bashrc
     ```

- Now you can call "lapce ." or "lapce --version" from cli

#### Fedora
2.1  install dependencies
```sh
sudo dnf install clang libxkbcommon-x11-devel libxcb-devel vulkan-loader-devel wayland-devel openssl-devel pkgconf
```

Follow instructions below to set path to lapce, where path to 
lapce is like: 
```
home/YOURNAME/lapce/target/release/lapce
```

2.1  Install Pearl (from official fedora source) to avoid a build-fail

https://developer.fedoraproject.org/tech/languages/perl/perl-installation.html
```bash
$ sudo dnf install perl-core
```


2.2 Create the ~/.local/bin directory
```bash
mkdir -p ~/.local/bin
```

2.3  Create a symbolic link:
- Open a terminal and run the following command to create a symbolic link from the executable to the chosen directory:
     ```bash
     ln -s home/YOURNAME/lapce/target/release/lapce ~/.local/bin/lapce
     ```

2.4  Add the directory to PATH (if necessary):
     ```bash
     export PATH="$PATH:$HOME/.local/bin"
     ```

2.5  Refresh the shell:
- Run the following command to reload the shell configuration:
     ```
     source ~/.bashrc
     ```

- Now you can call "lapce ." or "lapce --version" from cli

#### Void Linux
```sh
sudo xbps-install -S base-devel clang libxkbcommon-devel vulkan-loader wayland-devel
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
cargo install --path . --bin lapce --profile release-lto --locked
```

> If you use a different distribution, and are having trouble finding appropriate dependencies, let us know in an issue!

Once Lapce is compiled, the executable will be available in `$HOME/.cargo/bin/lapce` and should be available in `PATH` automatically.

## Building using Docker or Podman

Packages available in releases are built using containers based on multi-stage Dockerfiles. To easily orchestrate builds, there is a `docker-bake.hcl` manifest in root of repository that defines all stages and targets.
If you want to build all packages for ubuntu, you can run `RELEASE_TAG_NAME=nightly docker buildx bake ubuntu` (`RELEASE_TAG_NAME` is a required environment variable used to tell what kind of release is being built as well as baking in the version itself).
To scope in to specific distribution version, you can define target with it's version counterpart from matrix, e.g. to build only Ubuntu Focal package, you can run `RELEASE_TAG_NAME=nightly docker buildx bake ubuntu-focal`.
Additionally to building multiple OS versions at the same time, Docker-based builds will also try to cross-compile Lapce for other architectures.
This does not require QEMU installed as it's done via true cross-compilation meaning `HOST` will run your native OS/CPU architecture and `TARGET` will be the wanted architecture, instead of spawning container that's running OS using `TARGET` architecture.

> ![WARNING]
> Do not run plain targets like `ubuntu` or `fedora` if you don't have very powerful machine, as it will spawn many concurrent jobs
> which will take a long time to build.
