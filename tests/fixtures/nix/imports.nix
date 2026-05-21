# Nix fixture: import patterns
let
  localLib = import ./lib.nix;
  nixpkgs = import <nixpkgs> {};
  pinned = import (fetchTarball "https://example.com/nixpkgs.tar.gz") {};
in
{
  inherit (nixpkgs) stdenv fetchurl;
  lib = localLib;
  pkgs = nixpkgs;
}
