## Building from source

It is easy to build Lapce from source on a GNU/Linux distribution. Cargo handles the build process, all you need to do, is ensure the correct dependencies are installed.

1. Install the Rust compiler and Cargo using [`rustup.rs`](https://rustup.rs/). If you already have the toolchain, ensure you are using version 1.64 or higher.

2. Clone this repository (this command will clone to your home directory):
```sh
git clone https://github.com/lapce/lapce.git ~/lapce
```

3. `cd` into the repository, and run the specific setup file for your distribution:

#### Ubuntu
```sh
./setups/apt.sh
```
#### Fedora
```sh
./setups/dnf.sh
```
#### Manjaro
```sh
./setups/pacman.sh
```
#### Void Linux
```sh
./setups/xbps-install.sh
```

4. Run the build command with the release flag
```sh
cargo install --path . --bin lapce --profile release-lto --locked
```

> If you use a different distribution, and are having trouble finding appropriate dependencies, let us know in an issue! Most distributions that are similar e.g. Linux Mint and Ubuntu should work with the same setup file. Again, let us know if they don't!

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
