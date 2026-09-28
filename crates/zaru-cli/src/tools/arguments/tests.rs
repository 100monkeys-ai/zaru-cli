// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for the wire contract: what a call's arguments are, and what a
//! refusal on that path may say.
//!
//! # The security-corpus case here is an absence
//!
//! `fs.write`'s `contents` field is a whole file, so the argument path is the
//! one place in this surface where **the value the harness is least entitled
//! to publish arrives as an argument**. Every refusal is asserted not to carry
//! it, by the raw value and by an ASCII core no escaping can alter
//! ([Verification lessons] §50 and §63), with an accepting arm asserting the
//! *field name* is named — because a refusal that rendered nothing at all
//! would satisfy the absence assertion and tell a reader nothing.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::tools::arguments::{ArgumentsRefused, Call, schema};
use crate::tools::execute::descriptors;
use crate::tools::fixtures::nonce;
use crate::tools::name::{FieldKind, ToolName};
use serde_json::Value;

/// A value shaped like a bearer token, ending in text an escaping formatter
/// would alter, so that an absence assertion is not satisfied by escaping.
fn planted() -> String {
    format!("nn_mcp_{}-e\u{301}\u{e9}\u{1f701}", nonce("argument"))
}

/// Everything before the first non-ASCII character, which no escaping alters.
fn ascii_core(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii())
        .map_or(value.len(), |(at, _)| at);
    &value[..end]
}

/// ADR-0011 D1's seven each declare exactly the fields the schema offers.
///
/// The mutant: retyping the `required` array beside `fields()` rather than
/// deriving it, which is how a schema and a parser start disagreeing.
#[test]
fn the_schema_offered_and_the_object_accepted_are_one_list() {
    let mut disagreements = Vec::new();
    for tool in ToolName::ALL {
        let rendered = schema(tool);
        let document: Value = serde_json::from_str(&rendered)
            .unwrap_or_else(|error| panic!("{tool}'s schema is not JSON: {error}; {rendered}"));

        if document["type"] != "object" {
            disagreements.push(format!("{tool}: type is {}", document["type"]));
        }
        if document["additionalProperties"] != false {
            disagreements.push(format!(
                "{tool}: additionalProperties is {}, so the schema permits what `Call::parse` \
                 refuses",
                document["additionalProperties"]
            ));
        }

        let required: Vec<&str> = document["required"]
            .as_array()
            .unwrap_or_else(|| panic!("{tool}: required is not an array"))
            .iter()
            .map(|value| value.as_str().unwrap_or_default())
            .collect();
        let must: Vec<&str> = tool
            .fields()
            .iter()
            .filter(|field| field.required)
            .map(|field| field.name)
            .collect();
        if required != must {
            disagreements.push(format!(
                "{tool}: required is {required:?} and the parser requires {must:?}"
            ));
        }

        let mut properties: Vec<&str> = document["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{tool}: properties is not an object"))
            .keys()
            .map(String::as_str)
            .collect();
        properties.sort_unstable();
        let mut expected: Vec<&str> = tool.fields().iter().map(|field| field.name).collect();
        expected.sort_unstable();
        if properties != expected {
            disagreements.push(format!(
                "{tool}: properties are {properties:?} and the parser takes {expected:?}"
            ));
        }
        for field in tool.fields() {
            let declared = match field.kind {
                FieldKind::Text => "string",
                FieldKind::Number => "integer",
                FieldKind::Flag => "boolean",
            };
            if document["properties"][field.name]["type"] != declared {
                disagreements.push(format!(
                    "{tool}: {} is not declared as {declared}",
                    field.name
                ));
            }
        }
        println!("{tool}  {rendered}");
    }
    assert!(
        disagreements.is_empty(),
        "the schema a model is shown and the object the parser accepts must be one list: {}",
        disagreements.join("; ")
    );
}

/// Every descriptor's `parameters` is JSON a provider client can parse.
///
/// **This is the check the collision of 2026-09-05 would have reddened.** The
/// surface offered `parameters: String::new()` for all seven; an empty string
/// is not JSON — measured, `EOF while parsing a value at line 1 column 0` —
/// and `zaru-cli`'s own Gemini client parses that field with `serde_json` and
/// treats a failure as a defect of whoever supplied it. So all seven would
/// have been refused by the first provider handed them.
///
/// The mutant is that one line: `parameters: String::new()`.
#[test]
fn every_descriptor_carries_parameters_a_provider_can_parse() {
    let offered = descriptors();
    assert_eq!(
        offered.len(),
        ToolName::ALL.len(),
        "the descriptors are the seven ADR-0011 D1 names"
    );
    let mut unparseable = Vec::new();
    for descriptor in &offered {
        match serde_json::from_str::<Value>(&descriptor.parameters) {
            Ok(Value::Object(_)) => {}
            Ok(other) => unparseable.push(format!(
                "{}: parameters are {other} rather than an object",
                descriptor.name
            )),
            Err(error) => unparseable.push(format!("{}: {error}", descriptor.name)),
        }
        assert!(
            !descriptor.description.is_empty(),
            "{}: ADR-0011 D1 gives every tool a sentence and it is transcribed as the description",
            descriptor.name
        );
    }
    assert!(
        unparseable.is_empty(),
        "a descriptor whose parameters are not a JSON object is refused by a provider client as a \
         defect of whoever supplied it: {unparseable:?}"
    );
}

/// Each of the seven is built from the fields the record's own row needs.
///
/// The mutant: taking the fields in declaration order without checking which
/// tool they belong to, which pairs `old` with `contents`.
#[test]
fn a_call_is_built_from_the_fields_the_tool_declares() {
    let read = Call::parse(ToolName::FsRead, r#"{"path":"src/main.rs"}"#).expect("a read parses");
    assert_eq!(
        read,
        Call::Read {
            path: String::from("src/main.rs"),
            start_line: None,
            line_count: None,
        }
    );
    // The two numbers are different so that swapping them shows.
    let ranged = Call::parse(
        ToolName::FsRead,
        r#"{"path":"src/main.rs","start_line":40,"line_count":7}"#,
    )
    .expect("a ranged read parses");
    assert_eq!(
        ranged,
        Call::Read {
            path: String::from("src/main.rs"),
            start_line: Some(40),
            line_count: Some(7),
        }
    );

    // JSON has one number type, and a provider may hand back 3.0 for the 3
    // its model wrote.
    let whole = Call::parse(ToolName::FsRead, r#"{"path":"a","start_line":3.0}"#)
        .expect("a whole number with a zero fraction parses");
    assert_eq!(
        whole,
        Call::Read {
            path: String::from("a"),
            start_line: Some(3),
            line_count: None,
        }
    );

    let list = Call::parse(ToolName::FsList, r#"{"path":"src"}"#).expect("a list parses");
    assert_eq!(
        list,
        Call::OnPath {
            tool: ToolName::FsList,
            path: String::from("src"),
        }
    );

    let write = Call::parse(ToolName::FsWrite, r#"{"path":"a.txt","contents":"hello"}"#)
        .expect("a write parses");
    assert_eq!(
        write,
        Call::Write {
            path: String::from("a.txt"),
            contents: String::from("hello"),
        }
    );

    // The three fields are deliberately given values that would still look
    // right if two of them were swapped, except for their content -- so the
    // assertion is about which field went where and not about arity.
    let edit = Call::parse(
        ToolName::FsEdit,
        r#"{"path":"a.txt","old":"BEFORE","new":"AFTER"}"#,
    )
    .expect("an edit parses");
    assert_eq!(
        edit,
        Call::Edit {
            path: String::from("a.txt"),
            old: String::from("BEFORE"),
            new: String::from("AFTER"),
            all: false,
        }
    );
    let every = Call::parse(
        ToolName::FsEdit,
        r#"{"path":"a.txt","old":"BEFORE","new":"AFTER","all":true}"#,
    )
    .expect("an edit of every occurrence parses");
    assert_eq!(
        every,
        Call::Edit {
            path: String::from("a.txt"),
            old: String::from("BEFORE"),
            new: String::from("AFTER"),
            all: true,
        }
    );

    let search = Call::parse(ToolName::FsSearch, r#"{"root":"src","needle":"todo"}"#)
        .expect("a search parses");
    assert_eq!(
        search,
        Call::Search {
            root: String::from("src"),
            needle: String::from("todo"),
        }
    );

    let run = Call::parse(ToolName::CmdRun, r#"{"command":"printf hi"}"#).expect("a run parses");
    assert_eq!(
        run,
        Call::Run {
            command: String::from("printf hi"),
        }
    );

    let fetch = Call::parse(ToolName::WebFetch, r#"{"url":"https://example.invalid"}"#)
        .expect("a fetch parses");
    assert_eq!(
        fetch,
        Call::Fetch {
            url: String::from("https://example.invalid"),
        }
    );

    // Every one of the seven, so a tool added to `ALL` without a parse arm is
    // a hole this check names rather than one it walks past.
    for tool in ToolName::ALL {
        let object: serde_json::Map<String, Value> = tool
            .fields()
            .iter()
            .filter(|field| field.required)
            .map(|field| (field.name.to_owned(), Value::String(String::from("x"))))
            .collect();
        let arguments = Value::Object(object).to_string();
        let call = Call::parse(tool, &arguments).unwrap_or_else(|refused| {
            panic!("{tool}: its own declared object was refused: {refused}")
        });
        assert_eq!(
            call.tool(),
            tool,
            "a parsed call reports the tool it was parsed for"
        );
    }
}

/// Arguments that are not the declared object are refused, naming the field.
///
/// # Each row names the refusal it must get, and that is the whole check
///
/// A first version asserted only that each row **was** refused. The mutant it
/// exists to catch — a shim wrapping a bare value as the first declared field,
/// which is exactly the backward-compatibility path the harness's pre-alpha
/// rule forbids — **survived it**, because a wrapped array is still refused,
/// one variant later, for being the wrong type. So the check was reading "some
/// refusal happened" where the question is "which mistake was the model told
/// about". [Verification lessons] §51: read a surviving mutant as a check
/// finding before reading it as a code finding.
///
/// The mutants, one per row: a shim accepting a bare value, ignoring an
/// unexpected field, defaulting a missing field to the empty string, and
/// accepting a number where a string is declared.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn arguments_that_are_not_the_declared_object_are_refused_naming_the_field() {
    /// Which refusal a row must produce, without carrying its payload.
    fn shape(refused: &ArgumentsRefused) -> &'static str {
        match refused {
            ArgumentsRefused::NotJson { .. } => "NotJson",
            ArgumentsRefused::NotAnObject { .. } => "NotAnObject",
            ArgumentsRefused::MissingField { .. } => "MissingField",
            ArgumentsRefused::WrongKind { .. } => "WrongKind",
            ArgumentsRefused::UnexpectedField { .. } => "UnexpectedField",
        }
    }

    let cases: [(ToolName, &str, &str, &str); 12] = [
        // Not JSON at all -- which is what a bare path was, before the
        // contract existed.
        (ToolName::FsRead, "src/main.rs", "NotJson", "a bare path"),
        // JSON, and a string rather than an object. This is the row a shim
        // would swallow: it is the exact shape the old contract sent.
        (
            ToolName::FsRead,
            r#""src/main.rs""#,
            "NotAnObject",
            "a JSON string",
        ),
        // JSON, and not an object.
        (
            ToolName::FsRead,
            r#"["src/main.rs"]"#,
            "NotAnObject",
            "an array",
        ),
        // The field the tool declares is absent.
        (
            ToolName::FsWrite,
            r#"{"contents":"hi"}"#,
            "MissingField",
            "no path",
        ),
        // A field the tool does not declare. A model that spells `contents`
        // as `content` would otherwise write an empty file and be told it
        // worked.
        (
            ToolName::FsWrite,
            r#"{"path":"a.txt","contents":"hi","content":"hi"}"#,
            "UnexpectedField",
            "an undeclared field",
        ),
        // Declared, and not a string.
        (ToolName::FsRead, r#"{"path":7}"#, "WrongKind", "a number"),
        // Declared as a whole number of 1 or more, and not one.
        (
            ToolName::FsRead,
            r#"{"path":"a","start_line":"7"}"#,
            "WrongKind",
            "a number as a string",
        ),
        (
            ToolName::FsRead,
            r#"{"path":"a","start_line":0}"#,
            "WrongKind",
            "line 0",
        ),
        (
            ToolName::FsRead,
            r#"{"path":"a","line_count":2.5}"#,
            "WrongKind",
            "half a line",
        ),
        (
            ToolName::FsRead,
            r#"{"path":"a","line_count":-3}"#,
            "WrongKind",
            "a negative count",
        ),
        // Declared as true or false, and not one.
        (
            ToolName::FsEdit,
            r#"{"path":"a","old":"b","new":"c","all":"yes"}"#,
            "WrongKind",
            "a word for a flag",
        ),
        (
            ToolName::FsEdit,
            r#"{"path":"a","old":"b"}"#,
            "MissingField",
            "no new",
        ),
    ];

    let mut wrong = Vec::new();
    for (tool, arguments, expected, what) in cases {
        let Err(refused) = Call::parse(tool, arguments) else {
            wrong.push(format!(
                "{what}: {arguments} was accepted as a call to {tool}"
            ));
            continue;
        };
        let got = shape(&refused);
        if got != expected {
            wrong.push(format!(
                "{what}: {arguments} was refused as {got} and the mistake the model made is \
                 {expected}"
            ));
        }
        let rendered = refused.to_string();
        if !rendered.contains(tool.as_str()) {
            wrong.push(format!(
                "{what}: the refusal does not name the tool: {rendered}"
            ));
        }
        println!("{tool} <- {arguments}\n    [{got}] {rendered}");
    }
    assert!(
        wrong.is_empty(),
        "every row must be refused for the mistake it actually carries, or a shim that turns one \
         mistake into another passes: {}",
        wrong.join("; ")
    );

    // The accepting arm. Without it every assertion above is satisfied by a
    // parser that refuses everything.
    for tool in ToolName::ALL {
        let object: serde_json::Map<String, Value> = tool
            .fields()
            .iter()
            .filter(|field| field.required)
            .map(|field| (field.name.to_owned(), Value::String(String::from("x"))))
            .collect();
        Call::parse(tool, &Value::Object(object).to_string())
            .unwrap_or_else(|refused| panic!("{tool}: the declared object must parse: {refused}"));
    }
}

/// **Security corpus.** No refusal on the argument path renders a value.
///
/// `fs.write`'s `contents` is a whole file, so this path carries the one
/// argument the harness is least entitled to publish. Each row plants the
/// value in a different position and asserts it is absent from the refusal by
/// the raw value **and** by an ASCII core no escaping can alter — the two arms
/// [Verification lessons] §50 and §63 require, because `{:?}` escapes a
/// combining mark and an assertion written against the value as typed is blind
/// to a rendering that published every byte of it.
///
/// The mutant: quoting the offending value in any of the five refusals, or
/// using `serde_json::Error`'s own `Display`, which renders the value for a
/// type error.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn no_argument_refusal_renders_the_value_it_refused() {
    let secret = planted();
    let core = ascii_core(&secret);
    assert!(
        !core.is_empty() && core != secret,
        "the fixture must be awkward on the axis the mutant moves: a value whose ASCII core is \
         the whole value cannot tell an escaping rendering from an absent one"
    );

    let rows: Vec<(ToolName, String, &str)> = vec![
        // Malformed JSON carrying the value.
        (
            ToolName::FsWrite,
            format!(r#"{{"path":"a.txt","contents":"{secret}""#),
            "unterminated",
        ),
        // A declared field missing, with the value present in another.
        (
            ToolName::FsWrite,
            serde_json::json!({ "contents": secret }).to_string(),
            "missing path",
        ),
        // The value as an undeclared field's value. The field NAME is
        // rendered and its value is not, which the accepting arm below pins.
        (
            ToolName::FsWrite,
            serde_json::json!({ "path": "a.txt", "contents": "hi", "extra": secret }).to_string(),
            "undeclared field",
        ),
        // The value inside an array where a string is declared.
        (
            ToolName::FsRead,
            serde_json::json!({ "path": [secret.clone()] }).to_string(),
            "wrong type",
        ),
        // Not an object at all, the whole document being the value.
        (
            ToolName::FsRead,
            serde_json::json!(secret).to_string(),
            "not an object",
        ),
    ];

    let mut leaks = Vec::new();
    for (tool, arguments, what) in &rows {
        let refused = Call::parse(*tool, arguments)
            .expect_err(&format!("{what}: this is not a call to {tool}"));
        for rendering in [format!("{refused}"), format!("{refused:?}")] {
            if rendering.contains(&secret) {
                leaks.push(format!("{what}: the raw value is in {rendering}"));
            }
            if rendering.contains(core) {
                leaks.push(format!("{what}: the ASCII core is in {rendering}"));
            }
        }
        println!("{what}: {refused}");
    }
    assert!(
        leaks.is_empty(),
        "a refusal on the argument path carried the value it refused, and `fs.write`'s value is a \
         whole file: {leaks:?}"
    );

    // The accepting arm, twice over. A refusal that rendered nothing would
    // satisfy every assertion above: the field's NAME must be named, and the
    // declared fields must be listed, or the model cannot correct the call.
    let refused = Call::parse(
        ToolName::FsWrite,
        &serde_json::json!({ "path": "a.txt", "contents": "hi", "extra": secret }).to_string(),
    )
    .expect_err("an undeclared field is refused");
    assert!(
        matches!(refused, ArgumentsRefused::UnexpectedField { .. }),
        "the refusal is about the field: {refused:?}"
    );
    let rendered = refused.to_string();
    assert!(
        rendered.contains("extra") && rendered.contains("contents"),
        "the refusal names the field that arrived and the fields the tool takes: {rendered}"
    );
}
