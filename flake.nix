{
  description = "Code Intelligence for Large Language Models";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    naersk.url = "github:nix-community/naersk";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, naersk, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        rust = pkgs.rust-bin.stable.latest.default;
        naersk' = pkgs.callPackage naersk {
          cargo = rust;
          rustc = rust;
        };
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [ rust rust-analyzer ];
        };
        packages.default = naersk'.buildPackage {
          pname = "codanna";
          version = "0.9.17";
          src = ./.;
          buildInputs = [ pkgs.openssl pkgs.onnxruntime ];
          nativeBuildInputs = [ pkgs.pkg-config ];
          cargoBuildOptions = x: x ++ [ "-p" "codanna" ];
          ORT_SKIP_DOWNLOAD = "1";
        };
      });
}
