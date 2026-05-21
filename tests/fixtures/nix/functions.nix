# Nix fixture: various function patterns
{
  # Simple curried lambda
  add = a: b: a + b;
  multiply = a: b: a * b;

  # Formals (destructuring)
  mkService = { name, port ? 8080, debug ? false }: {
    inherit name port debug;
  };

  # Formals with ellipsis
  mkPkg = { name, src, buildInputs ? [], ... }: derivation {
    inherit name src buildInputs;
    system = builtins.currentSystem;
    builder = "/bin/sh";
  };

  # @ pattern
  withExtras = args @ { name, ... }: {
    fullArgs = args;
    inherit name;
  };

  # Higher-order function
  compose = f: g: x: f (g x);

  # Nested lambdas
  applyTwice = f: x: f (f x);
}
