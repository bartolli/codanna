# Comprehensive Nix example — one attrset at the top level (valid Nix)
{
  # --- simple lambda ---
  identity = x: x;

  # --- nested lambda (curried) ---
  add = a: b: a + b;

  # --- attrset binding ---
  config = {
    host = "localhost";
    port = 8080;
    debug = false;
  };

  # --- rec attrset (self-referential) ---
  defaults = rec {
    base = "/var/lib";
    data = "${base}/data";
    logs = "${base}/logs";
  };

  # --- let expression ---
  result =
    let
      x = 10;
      y = 20;
      inner = v: v * 2;
    in
      inner x + y;

  # --- formals with defaults and ellipsis ---
  mkService = { name, port ? 8080, debug ? false, ... }: {
    inherit name port debug;
    description = "Service: ${name}";
  };

  # --- @ pattern (bind whole set + named fields) ---
  mkDerivation = args @ { name, src, buildInputs ? [], ... }:
    derivation {
      inherit name src buildInputs;
      system = builtins.currentSystem;
      builder = "/bin/sh";
    };

  # --- inherit from source ---
  pkgAttrs =
    let pkgs = import <nixpkgs> {};
    in {
      inherit (pkgs) stdenv fetchurl;
      lib = pkgs.lib;
    };

  # --- with expression ---
  withExample = with builtins; [
    (toString 42)
    (typeOf "hello")
    (length [ 1 2 3 ])
  ];

  # --- if expression ---
  classify = n:
    if n < 0 then "negative"
    else if n == 0 then "zero"
    else "positive";

  # --- assert ---
  safeDivide = a: b:
    assert b != 0;
    a / b;

  # --- import with path ---
  nixpkgsLib = import <nixpkgs/lib>;

  # --- select expression (attrpath) ---
  version = builtins.currentSystem;

  # --- list ---
  items = [ 1 2 3 "four" true ];

  # --- string interpolation ---
  greeting = name: "Hello, ${name}!";

  # --- multiline string ---
  script = ''
    #!/bin/bash
    echo "hello"
    exit 0
  '';

  # --- path expression ---
  configPath = /etc/nixos/configuration.nix;

  # --- spath (angle-bracket path) ---
  nixpkgsPath = <nixpkgs>;

  # --- inherit without source ---
  passThrough = { a, b, c }: {
    inherit a b c;
  };

  # --- nested attrset access (select_expression) ---
  deep = {
    a = {
      b = {
        c = 42;
      };
    };
  };

  # --- function as attrset value (common nixpkgs pattern) ---
  lib = {
    mkOption = { type, default, description ? "" }: {
      _type = "option";
      inherit type default description;
    };

    types = rec {
      str = { name = "str"; check = builtins.isString; };
      int = { name = "int"; check = builtins.isInt; };
      listOf = element: {
        name = "listOf";
        check = x: builtins.isList x;
      };
    };
  };
}
