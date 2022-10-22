{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    crane.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {inherit system;};
        inherit (nixpkgs) lib;
        craneLib = crane.lib.${system};
      in
    {
      # Based on https://github.com/NixOS/nixpkgs/blob/master/pkgs/applications/editors/lapce/default.nix
      packages.default = craneLib.buildPackage rec {
        src = ./.;
        OPENSSL_NO_VENDOR = 1;
        doCheck = true;
        
        buildInputs = with pkgs; [glib gtk3 openssl]
          ++ lib.optionals stdenv.isLinux [fontconfig]
          ++ lib.optionals stdenv.isDarwin [
            libobjc
            Security
            CoreServices
            ApplicationServices
            Carbon
            AppKit
          ];
        nativeBuildInputs = with pkgs; [cmake gcc pkg-config];
        
        postInstall = ''
          install -Dm0644 ${src}/extra/images/logo.svg $out/share/icons/hicolor/scalable/apps/lapce.svg
        '';

        desktopItems = [ (pkgs.makeDesktopItem {
          name = "lapce";
          exec = "lapce %F";
          icon = "lapce";
          desktopName = "Lapce";
          comment = "Lightning-fast and Powerful Code Editor written in Rust";
          genericName = "Code Editor";
          categories = [ "Development" "Utility" "TextEditor" ];
        }) ];
      };
    });
}