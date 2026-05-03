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
        libraries = with pkgs; [
          webkitgtk_4_1
          gtk3
          cairo
          gdk-pixbuf
          glib
          dbus
          librsvg
          openssl
          libappindicator-gtk3
          libayatana-appindicator
          mpv
          libglvnd
          
          # GStreamer for WebKitGTK media (video/audio capture and playback)
          gst_all_1.gstreamer
          gst_all_1.gst-plugins-base
          gst_all_1.gst-plugins-good
          gst_all_1.gst-plugins-bad
          gst_all_1.gst-plugins-ugly
          gst_all_1.gst-libav
        ];

        gstPluginPath = pkgs.lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" (with pkgs.gst_all_1; [ 
          gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav 
        ]);

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
            git
            openssl
          ];

          buildInputs = libraries ++ (with pkgs; [
            gst_all_1.gstreamer.dev
            gtk-layer-shell.dev
          ]);

          # Combined fix for Compilation and Runtime
          shellHook = ''
            # Disables the DMA-BUF renderer in webkit (causes crashes/weird behavior)
            export WEBKIT_DISABLE_DMABUF_RENDERER=1
            
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath libraries}:$LD_LIBRARY_PATH"
            export XDG_DATA_DIRS="$GSETTINGS_SCHEMAS_PATH"
            export JAVA_HOME="${pkgs.zulu.home}"
            
            # Allow GStreamer to find plugins for media processing
            export GST_PLUGIN_SYSTEM_PATH_1_0="${gstPluginPath}"
            export GST_PLUGIN_PATH_1_0="${gstPluginPath}"
            export GST_PLUGIN_SCANNER="${pkgs.gst_all_1.gstreamer.out}/libexec/gstreamer-1.0/gst-plugin-scanner"
            
            # VA-API support (Nvidia specific as per suggestion)
            export LIBVA_DRIVERS_PATH="/run/opengl-driver/lib/dri"
            export LIBVA_DRIVER_NAME="nvidia"
            
            echo "RAMS Dev Environment: WebKit DMA-BUF Fixed + GStreamer + Nvidia VA-API"
          '';
        };
      }
    );
}