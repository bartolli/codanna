# Entry point example — a minimal NixOS/nixpkgs-style package set
let
  pkgs = import <nixpkgs> {};
in
{
  # A simple derivation
  hello = pkgs.stdenv.mkDerivation {
    name = "hello-1.0";
    src = ./src;
    buildPhase = ''
      gcc -o hello main.c
    '';
    installPhase = ''
      mkdir -p $out/bin
      cp hello $out/bin/
    '';
  };

  # A shell for development
  devShell = pkgs.mkShell {
    buildInputs = [ pkgs.gcc pkgs.gnumake ];
    shellHook = ''
      echo "Dev shell ready"
    '';
  };
}
