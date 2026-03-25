{
  description = "Code Intelligence for Large Language Models";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      naersk,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
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
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rust
            rust-analyzer
            openssl
            # Must match ort crate pin in Cargo.toml (ort =2.0.0-rc.10 -> ONNX Runtime 1.22).
            # ABI mismatch causes silent runtime corruption. Verify after nixpkgs updates.
            onnxruntime
            pkg-config
          ];
          ORT_SKIP_DOWNLOAD = "1";
        };
        packages.default = naersk'.buildPackage {
          pname = "codanna";
          src = ./.;
          buildInputs = [
            pkgs.openssl
            # Must match ort crate pin in Cargo.toml (ort =2.0.0-rc.10 -> ONNX Runtime 1.22).
            # ABI mismatch causes silent runtime corruption. Verify after nixpkgs updates.
            pkgs.onnxruntime
          ];
          nativeBuildInputs = [ pkgs.pkg-config ];
          cargoBuildOptions =
            x:
            x
            ++ [
              "-p"
              "codanna"
            ];
          ORT_SKIP_DOWNLOAD = "1";
        };
      }
    );
}
