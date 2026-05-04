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

        # Optimization: Group libraries needed for Tauri's webview and media
        # Note: If using Tauri v2, webkitgtk_4_1 is correct. For v1, use webkitgtk.
        libraries = with pkgs; [
          webkitgtk_4_1
          gtk3
          cairo
          gdk-pixbuf
          glib
          dbus
          openssl
          librsvg
          libappindicator-gtk3
          libsoup_3
          harfbuzz
          pango
          atk
        ];

        packages = with pkgs; [
          curl
          wget
          pkg-config
          dbus
          openssl
          glib
          gtk3
          libsoup_3
          webkitgtk_4_1
          librsvg
          # GStreamer
          gst_all_1.gstreamer
          gst_all_1.gst-plugins-base
          gst_all_1.gst-plugins-good
          gst_all_1.gst-plugins-bad
          gst_all_1.gst-plugins-ugly
          gst_all_1.gst-libav
        ];

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "clippy" "rustfmt" "rust-analyzer" ];
          targets = [ "x86_64-unknown-linux-gnu" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          # nativeBuildInputs: tools needed at compile time
          nativeBuildInputs = with pkgs; [
            openssl
            pkg-config
            copyDesktopItems
            rustToolchain
            cargo-tauri
            bun
          ];

          # buildInputs: libraries needed at linked/runtime
          buildInputs = packages;

          shellHook = ''
            # Fixes "Could not find library gdk-3.0" during cargo build
            export PKG_CONFIG_PATH="${pkgs.lib.makeSearchPathOutput "dev" "lib/pkgconfig" packages}"
            
            # Ensures WebKit and GTK can find their schemas and assets
            export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS"
            
            # Disables the DMA-BUF renderer (avoids the "black screen" or crash on Nvidia)
            export WEBKIT_DISABLE_DMABUF_RENDERER=1
            
            # Dynamic Linker Fix (Excluding OpenSSL to avoid breaking system tools like git)
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath (pkgs.lib.filter (p: p != pkgs.openssl) libraries)}:$LD_LIBRARY_PATH"
            
            # GStreamer Setup
            export GST_PLUGIN_SYSTEM_PATH_1_0="${pkgs.lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" [
              pkgs.gst_all_1.gstreamer
              pkgs.gst_all_1.gst-plugins-base
              pkgs.gst_all_1.gst-plugins-good
              pkgs.gst_all_1.gst-plugins-bad
              pkgs.gst_all_1.gst-plugins-ugly
              pkgs.gst_all_1.gst-libav
            ]}"

            echo "Tauri Dev Environment Loaded (NixOS)"
            echo "Note: If you see GLib-GIO-ERROR, ensure gsettings-desktop-schemas is in your system packages."
          '';
        };
      }
    );
}