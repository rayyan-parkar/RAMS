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
          openssl_3
          librsvg
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
            # nodejs_20 # Bun is great, but sometimes Tauri scripts expect node
          ];

          buildInputs = libraries;

          # Essential for Rust to find libraries during compilation
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libraries;

          shellHook = ''
            export JAVA_HOME="${pkgs.zulu.home}"
            # This ensures GSettings and themes work for the Tauri window
            export XDG_DATA_DIRS="$GSETTINGS_SCHEMAS_PATH"
            echo "Rust and Tauri Shell Loaded"
          '';
        };
      }
    );
}
