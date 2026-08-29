//! php binding names carry the `$` sigil, matching the receiver token on call rows.
//!
//! `extract_method_calls_from_node` captures the receiver as the raw source
//! token (`$h`). `binding_type_at_call_site` joins a binding to a call by
//! `binding.name == receiver`, so a binding recorded as `h` never matches and
//! no php variable receiver ever infers a type. php's own
//! `self_receiver_aliases()` is `["$this"]` — the language is committed to
//! sigil-carrying receiver tokens, and the binding channel is the outlier.

use codanna::parsing::LanguageParser;
use codanna::parsing::php::PhpParser;

fn parser() -> PhpParser {
    PhpParser::new().expect("php parser")
}

#[test]
fn typed_parameter_binding_keeps_the_sigil() {
    let code = r#"<?php

class HubPhp
{
    public function ping(): void {}
}

function seed(HubPhp $h): void
{
    $h->ping();
}
"#;

    let bindings = parser().find_variable_types(code);

    let hit = bindings
        .iter()
        .find(|(name, _, _)| name.trim_start_matches('$') == "h")
        .expect("the typed parameter must produce a binding");

    assert_eq!(
        hit.0, "$h",
        "binding name must be the source token so it joins the call row's receiver"
    );
    assert_eq!(hit.1, "HubPhp", "binding type is the annotated type");
}

#[test]
fn multiple_typed_parameters_all_keep_the_sigil() {
    let code = r#"<?php

function seed(HubPhp $first, OtherPhp $second): void
{
    $first->ping();
    $second->pong();
}
"#;

    let bindings = parser().find_variable_types(code);
    let names: Vec<&str> = bindings.iter().map(|(n, _, _)| *n).collect();

    assert!(
        names.contains(&"$first") && names.contains(&"$second"),
        "every typed parameter binds under its source token, got {names:?}"
    );
}
