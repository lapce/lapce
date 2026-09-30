Name:           lapce-git
Version:        0.4.6.{{{ git_dir_version }}}
Release:        1
Summary:        Lightning-fast and Powerful Code Editor written in Rust
License:        Apache-2.0
URL:            https://github.com/lapce/lapce

VCS:            {{{ git_dir_vcs }}}
Source:        	{{{ git_dir_pack }}}

BuildRequires:  cargo libxkbcommon-x11-devel libxcb-devel vulkan-loader-devel wayland-devel openssl-devel pkgconf libxkbcommon-x11-devel

%description
Lapce is written in pure Rust, with a UI in Floem (also written in Rust).
It is designed with Rope Science from the Xi-Editor, enabling lightning-fast computation, and leverages wgpu for rendering.

%prep
{{{ git_dir_setup_macro }}}
cargo fetch --locked

%build
cargo build --profile release-lto --package lapce-app --frozen

%install
install -Dm755 target/release-lto/lapce %{buildroot}%{_bindir}/lapce
install -Dm644 extra/linux/dev.lapce.lapce.desktop %{buildroot}/usr/share/applications/dev.lapce.lapce.desktop
install -Dm644 extra/linux/dev.lapce.lapce.metainfo.xml %{buildroot}/usr/share/metainfo/dev.lapce.lapce.metainfo.xml
install -Dm644 extra/images/logo.png %{buildroot}/usr/share/pixmaps/dev.lapce.lapce.png

%files
%license LICENSE*
%doc *.md
%{_bindir}/lapce
/usr/share/applications/dev.lapce.lapce.desktop
/usr/share/metainfo/dev.lapce.lapce.metainfo.xml
/usr/share/pixmaps/dev.lapce.lapce.png

%changelog
* Mon Jan 01 2024 Jakub Panek
- See full changelog on GitHub
