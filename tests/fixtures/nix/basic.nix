# Basic Nix fixture: simple attrset with functions and values
{
  # Plain value bindings
  host = "localhost";
  port = 8080;
  debug = false;

  # Lambda bindings
  identity = x: x;
  add = a: b: a + b;
  greet = name: "Hello, ${name}";

  # Nested attrset
  config = {
    timeout = 30;
    retries = 3;
  };

  # Let expression
  computed =
    let
      base = 10;
      factor = 2;
    in
      base * factor;
}
