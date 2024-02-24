Name:           lapce-git
Version:        0.3.1.{{{ git_dir_version }}}
Release:        1
Summary:        Lightning-fast and Powerful Code Editor written in Rust
License:        Apache-2.0
URL:            https://github.com/lapce/lapce

VCS:            {{{ git_dir_vcs }}}
Source:        	{{{ git_dir_pack }}}

BuildRequires:  cargo perl-FindBin cairo-devel cairo-gobject-devel atk-devel gdk-pixbuf2-devel pango-devel gtk3-devel perl-lib perl-File-Compare mold clang libxkbcommon-x11-devel

%description
Lapce is written in pure Rust with a UI in Druid (which is also written in Rust).
It is designed with Rope Science from the Xi-Editor which makes for lightning-fast computation, and leverages OpenGL for rendering.

%prep
{{{ git_dir_setup_macro }}}

%build
RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold" cargo build --profile release-lto

%install
install -Dm755 target/release-lto/lapce %{buildroot}%{_bindir}/lapce
install -Dm755 target/release-lto/lapce-proxy %{buildroot}%{_bindir}/lapce-proxy
install -Dm644 extra/linux/dev.lapce.lapce.desktop %{buildroot}/usr/share/applications/dev.lapce.lapce.desktop
install -Dm644 extra/linux/dev.lapce.lapce.metainfo.xml %{buildroot}/usr/share/metainfo/dev.lapce.lapce.metainfo.xml
install -Dm644 extra/images/logo.png %{buildroot}/usr/share/pixmaps/dev.lapce.lapce.png

%files
%license LICENSE*
%doc *.md
%{_bindir}/lapce
%{_bindir}/lapce-proxy
/usr/share/applications/dev.lapce.lapce.desktop
/usr/share/metainfo/dev.lapce.lapce.metainfo.xml
/usr/share/pixmaps/dev.lapce.lapce.png

%changelog
* Sat Jul 16 2022 Simon Garding <titaniumtown@gmail.com> - test
- test
