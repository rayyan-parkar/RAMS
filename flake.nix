{
  description = "Rust + Tauri + WebRTC Development Environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
          config.allowUnfree = true;
        };

        # Optimization: Group libraries needed for Tauri's webview
        libraries = with pkgs; [
          webkitgtk_4_1
          gtk3
          cairo
          gdk-pixbuf
          glib
          dbus
          librsvg
          openssl # Use one consistent version
        ];

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "clippy" "rustfmt" "rust-analyzer" ];
          targets = [ "x86_64-unknown-linux-gnu" "aarch64-linux-android" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            pkg-config
            wrapGAppsHook4
            rustToolchain
            cargo-tauri
            bun
            git      # CRITICAL: Adds Git to the environment so it uses the flake's OpenSSL
            openssl
          ];

          buildInputs = libraries;

          # Combined fix for Compilation and Runtime
          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath libraries}:$LD_LIBRARY_PATH"
            export XDG_DATA_DIRS="$GSETTINGS_SCHEMAS_PATH"
            export JAVA_HOME="${pkgs.zulu.home}"
            echo "RAMS Dev Environment: Git + Rust + Tauri Loaded"
          '';
        };
      }
    );
}