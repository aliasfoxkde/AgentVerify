//! Tests for the contract error-diagnostic API: source locations, contexts,
//! error rendering and chaining, file-format dispatch, and the JSON/YAML
//! serialization helpers.
//!
//! The parse happy paths are covered by unit tests inside the module; these
//! tests pin the behavior callers rely on when a contract is *rejected*.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use agentverify_contract::{
    parse_json, parse_yaml, to_json, to_yaml, validate_contract, Contract, ContractContext,
    ContractError, PredicatePath, SourceLocation,
};
use agentverify_core::{ContractId, ContractValidationError, Predicate};

const VALID_JSON: &str = r#"{
    "action_name": "create_customer",
    "postconditions": [
        {
            "predicate": {"type": "exists", "path": "customer.id"},
            "description": "Customer was created"
        }
    ]
}"#;

const VALID_YAML: &str = "action_name: create_customer
postconditions:
  - predicate:
      type: exists
      path: customer.id
    description: Customer was created
";

// ---------------------------------------------------------------------------
// SourceLocation
// ---------------------------------------------------------------------------

#[test]
fn source_location_renders_without_column() {
    let location = SourceLocation::new("contracts/close-ticket.json", 12);
    assert_eq!(location.to_string(), "contracts/close-ticket.json:12");
}

#[test]
fn source_location_renders_with_column() {
    let location = SourceLocation::with_column("contracts/close-ticket.json", 12, 5);
    assert_eq!(location.to_string(), "contracts/close-ticket.json:12:5");
}

// ---------------------------------------------------------------------------
// ContractContext and PredicatePath
// ---------------------------------------------------------------------------

#[test]
fn context_with_action_renders_the_action_name() {
    let context = ContractContext::with_action("create_customer");
    assert_eq!(context.to_string(), "action_name=create_customer");
    assert_eq!(context.contract_id, None);
    assert_eq!(context.action_name.as_deref(), Some("create_customer"));
}

#[test]
fn context_with_contract_id_renders_the_id() {
    let id = ContractId::new();
    let context = ContractContext::with_contract_id(id);
    assert_eq!(context.action_name, None);
    let rendered = context.to_string();
    assert!(
        rendered.starts_with("contract_id="),
        "unexpected rendering: {rendered}"
    );
    assert_eq!(
        rendered,
        format!("contract_id={}", context.contract_id.unwrap())
    );
}

#[test]
fn predicate_path_renders_with_an_at_prefix() {
    let path = PredicatePath {
        path: "postconditions[0].not".to_string(),
    };
    assert_eq!(path.to_string(), "at postconditions[0].not");
}

// ---------------------------------------------------------------------------
// ContractError rendering and chaining
// ---------------------------------------------------------------------------

fn json_error() -> ContractError {
    let err = serde_json::from_str::<Contract>("{ not json").unwrap_err();
    ContractError::JsonError {
        source: err,
        location: None,
        context: None,
    }
}

fn yaml_error() -> ContractError {
    let err = serde_yaml::from_str::<Contract>("action_name: [unclosed").unwrap_err();
    ContractError::YamlError {
        source: err,
        location: None,
        context: None,
    }
}

fn io_error() -> ContractError {
    ContractError::IoError {
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
        location: None,
        context: None,
    }
}

fn invalid_contract_error() -> ContractError {
    ContractError::InvalidContract {
        reason: "Contract must have at least one postcondition".to_string(),
        location: None,
        context: None,
    }
}

fn invalid_predicate_error() -> ContractError {
    ContractError::InvalidPredicate {
        reason: "All predicate must have at least one predicate".to_string(),
        path: PredicatePath {
            path: "postconditions[0]".to_string(),
        },
        context: None,
    }
}

fn unknown_extension_error() -> ContractError {
    ContractError::UnknownFileExtension {
        extension: "toml".to_string(),
        location: None,
        context: None,
    }
}

#[test]
fn every_error_variant_renders_a_stable_message() {
    let location = SourceLocation::with_column("c.json", 3, 7);
    let context = ContractContext::with_action("create_customer");

    // Bare (no location, no context).
    assert!(json_error()
        .to_string()
        .starts_with("Failed to parse JSON: "));
    assert!(yaml_error()
        .to_string()
        .starts_with("Failed to parse YAML: "));
    assert!(io_error()
        .to_string()
        .starts_with("Failed to read file: no such file"));
    assert_eq!(
        invalid_contract_error().to_string(),
        "Invalid contract: Contract must have at least one postcondition"
    );
    assert_eq!(
        invalid_predicate_error().to_string(),
        "Invalid predicate at postconditions[0]: All predicate must have at least one predicate"
    );
    assert_eq!(
        unknown_extension_error().to_string(),
        "Unknown file extension: toml"
    );

    // Decorated with location and context where the variant supports it.
    let decorated = ContractError::InvalidContract {
        reason: "action_name cannot be empty".to_string(),
        location: Some(location.clone()),
        context: Some(context.clone()),
    };
    assert_eq!(
        decorated.to_string(),
        "Invalid contract (c.json:3:7) [action_name=create_customer]: \
         action_name cannot be empty"
    );

    let located_json = match json_error() {
        ContractError::JsonError { source, .. } => ContractError::JsonError {
            source,
            location: Some(location),
            context: Some(context),
        },
        other => panic!("wrong variant: {other:?}"),
    };
    let rendered = located_json.to_string();
    assert!(
        rendered.starts_with("Failed to parse JSON (c.json:3:7) [action_name=create_customer]: "),
        "unexpected rendering: {rendered}"
    );
}

#[test]
fn every_variant_decorates_with_location_and_context() {
    let location = SourceLocation::with_column("svc/close.yaml", 9, 2);
    let context = ContractContext::with_action("create_customer");

    let decorated_yaml = match yaml_error() {
        ContractError::YamlError { source, .. } => ContractError::YamlError {
            source,
            location: Some(location.clone()),
            context: Some(context.clone()),
        },
        other => panic!("wrong variant: {other:?}"),
    };
    let rendered = decorated_yaml.to_string();
    assert!(
        rendered.starts_with(
            "Failed to parse YAML (svc/close.yaml:9:2) [action_name=create_customer]: "
        ),
        "unexpected rendering: {rendered}"
    );

    let decorated_io = match io_error() {
        ContractError::IoError { source, .. } => ContractError::IoError {
            source,
            location: Some(location.clone()),
            context: Some(context.clone()),
        },
        other => panic!("wrong variant: {other:?}"),
    };
    let rendered = decorated_io.to_string();
    assert!(
        rendered.starts_with(
            "Failed to read file: no such file (svc/close.yaml:9:2) \
             [action_name=create_customer]"
        ),
        "unexpected rendering: {rendered}"
    );

    let decorated_unknown = ContractError::UnknownFileExtension {
        extension: "toml".to_string(),
        location: Some(location.clone()),
        context: Some(context.clone()),
    };
    assert_eq!(
        decorated_unknown.to_string(),
        "Unknown file extension: toml (svc/close.yaml:9:2) \
         [action_name=create_customer]"
    );

    // InvalidPredicate renders its context between the prefix and the path.
    let contextual_predicate = ContractError::InvalidPredicate {
        reason: "Any predicate must have at least one predicate".to_string(),
        path: PredicatePath {
            path: "postconditions[1]".to_string(),
        },
        context: Some(context),
    };
    assert_eq!(
        contextual_predicate.to_string(),
        "Invalid predicate [action_name=create_customer] at postconditions[1]: \
         Any predicate must have at least one predicate"
    );
}

#[test]
fn source_chains_through_the_wrapping_variants() {
    use std::error::Error;

    let json = json_error();
    assert!(json.source().is_some());
    assert!(json
        .source()
        .unwrap()
        .downcast_ref::<serde_json::Error>()
        .is_some());

    let yaml = yaml_error();
    assert!(yaml
        .source()
        .unwrap()
        .downcast_ref::<serde_yaml::Error>()
        .is_some());

    let io = io_error();
    assert!(io
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .is_some());

    // Structural errors have no deeper source.
    assert!(invalid_contract_error().source().is_none());
    assert!(invalid_predicate_error().source().is_none());
    assert!(unknown_extension_error().source().is_none());
}

#[test]
fn with_location_fills_only_when_unset() {
    let existing = SourceLocation::new("original.yaml", 1);
    let replacement = SourceLocation::new("replacement.yaml", 2);

    // Unset locations are filled for every variant that carries one.
    for err in [
        json_error(),
        yaml_error(),
        io_error(),
        invalid_contract_error(),
        unknown_extension_error(),
    ] {
        let located = err.with_location(replacement.clone());
        let rendered = located.to_string();
        assert!(
            rendered.contains("replacement.yaml"),
            "location not attached: {rendered}"
        );
    }

    // An existing location is preserved, not overwritten.
    let preserved = match invalid_contract_error() {
        ContractError::InvalidContract { reason, .. } => ContractError::InvalidContract {
            reason,
            location: Some(existing.clone()),
            context: None,
        },
        other => panic!("wrong variant: {other:?}"),
    }
    .with_location(replacement.clone());
    assert!(preserved.to_string().contains("original.yaml"));
    assert!(!preserved.to_string().contains("replacement.yaml"));

    // Variants without a location field pass through untouched.
    let untouched = invalid_predicate_error().with_location(replacement);
    assert_eq!(untouched.to_string(), invalid_predicate_error().to_string());
}

#[test]
fn with_context_fills_only_when_unset() {
    let context = ContractContext::with_action("create_customer");
    let other = ContractContext::with_action("other_action");

    for err in [
        json_error(),
        yaml_error(),
        io_error(),
        invalid_contract_error(),
        invalid_predicate_error(),
        unknown_extension_error(),
    ] {
        let contextual = err.with_context(context.clone());
        assert_eq!(
            contextual.action_name(),
            Some("create_customer"),
            "context not attached to {contextual}"
        );
    }

    // An existing context is preserved.
    let preserved = io_error().with_context(context.clone()).with_context(other);
    assert_eq!(preserved.action_name(), Some("create_customer"));
}

#[test]
fn accessors_surface_the_context_fields() {
    let id = ContractId::new();
    let both = ContractContext {
        contract_id: Some(id),
        action_name: Some("create_customer".to_string()),
    };

    let err = io_error().with_context(both);
    assert_eq!(err.contract_id(), Some(&id));
    assert_eq!(err.action_name(), Some("create_customer"));

    // Without a context both accessors are empty.
    assert_eq!(invalid_contract_error().contract_id(), None);
    assert_eq!(invalid_contract_error().action_name(), None);
}

// ---------------------------------------------------------------------------
// Parsing and loading failure paths
// ---------------------------------------------------------------------------

#[test]
fn parse_json_rejects_malformed_json() {
    let err = parse_json("{ not json").unwrap_err();
    assert!(matches!(err, ContractError::JsonError { .. }));
    assert!(err.to_string().starts_with("Failed to parse JSON"));
}

#[test]
fn parse_yaml_rejects_malformed_yaml() {
    let err = parse_yaml("action_name: [unclosed").unwrap_err();
    assert!(matches!(err, ContractError::YamlError { .. }));
    assert!(err.to_string().starts_with("Failed to parse YAML"));
}

#[test]
fn validate_rejects_wrong_schema_version() {
    // Parsing accepts any declared version; the semantic validation on the
    // contract itself (not the structural `validate_contract`) rejects it.
    let contract = parse_json(
        r#"{
        "schema_version": "9.9",
        "action_name": "create_customer",
        "postconditions": [
            {"predicate": {"type": "exists", "path": "customer.id"}, "description": "ok"}
        ]
    }"#,
    )
    .unwrap();
    let err = contract.validate().unwrap_err();
    match &err {
        ContractValidationError::IncompatibleSchemaVersion { expected, actual } => {
            assert_eq!(expected, "1.0");
            assert_eq!(actual, "9.9");
        }
        other => panic!("wrong variant: {other:?}"),
    }
    assert!(err.to_string().contains("chema version"));
}

#[test]
fn parse_rejects_empty_all_predicates_with_a_path() {
    let err = parse_json(
        r#"{
        "action_name": "create_customer",
        "postconditions": [
            {"predicate": {"type": "all", "predicates": []}, "description": "ok"}
        ]
    }"#,
    )
    .unwrap_err();
    match &err {
        ContractError::InvalidPredicate { path, .. } => {
            assert_eq!(path.to_string(), "at postconditions[0]");
        }
        other => panic!("wrong variant: {other:?}"),
    }
    assert!(err
        .to_string()
        .contains("All predicate must have at least one"));
}

#[test]
fn parse_rejects_empty_any_and_not_traverses_nested_paths() {
    // `not` wraps an empty `any`, so the reported path descends into it.
    let err = parse_json(
        r#"{
        "action_name": "create_customer",
        "preconditions": [
            {"predicate": {"type": "not", "predicate": {"type": "any", "predicates": []}}}
        ],
        "postconditions": [
            {"predicate": {"type": "exists", "path": "customer.id"}, "description": "ok"}
        ]
    }"#,
    )
    .unwrap_err();
    match &err {
        ContractError::InvalidPredicate { path, .. } => {
            assert_eq!(path.to_string(), "at preconditions[0].not");
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn load_file_reports_missing_files_as_io_errors() {
    let err = agentverify_contract::load_file("definitely/not/here.json").unwrap_err();
    match &err {
        ContractError::IoError { location, .. } => {
            assert_eq!(location.as_ref().unwrap().file, "definitely/not/here.json");
        }
        other => panic!("wrong variant: {other:?}"),
    }
    assert!(err.to_string().starts_with("Failed to read file"));
}

#[test]
fn load_file_rejects_unknown_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("contract.toml");
    std::fs::write(&path, "[section]").unwrap();

    let err = agentverify_contract::load_file(&path).unwrap_err();
    match &err {
        ContractError::UnknownFileExtension { extension, .. } => {
            assert_eq!(extension, "toml");
        }
        other => panic!("wrong variant: {other:?}"),
    }
    assert!(err.to_string().contains("Unknown file extension: toml"));
}

#[test]
fn load_file_dispatches_by_extension_and_attaches_locations() {
    let dir = tempfile::tempdir().unwrap();

    let json_path = dir.path().join("good.json");
    std::fs::write(&json_path, VALID_JSON).unwrap();
    assert_eq!(
        agentverify_contract::load_file(&json_path)
            .unwrap()
            .action_name,
        "create_customer"
    );

    let yaml_path = dir.path().join("good.yaml");
    std::fs::write(&yaml_path, VALID_YAML).unwrap();
    assert_eq!(
        agentverify_contract::load_file(&yaml_path)
            .unwrap()
            .action_name,
        "create_customer"
    );

    let yml_path = dir.path().join("good.yml");
    std::fs::write(&yml_path, VALID_YAML).unwrap();
    assert_eq!(
        agentverify_contract::load_file(&yml_path)
            .unwrap()
            .action_name,
        "create_customer"
    );

    // A broken YAML file under .yaml attaches the file location to the
    // parse error.
    let broken_path = dir.path().join("broken.yaml");
    std::fs::write(&broken_path, "action_name: [unclosed").unwrap();
    let err = agentverify_contract::load_file(&broken_path).unwrap_err();
    assert!(
        err.to_string().contains("broken.yaml"),
        "location not attached: {err}"
    );
}

#[test]
fn load_file_without_extension_tries_json_then_yaml() {
    let dir = tempfile::tempdir().unwrap();

    let json_path = dir.path().join("contract");
    std::fs::write(&json_path, VALID_JSON).unwrap();
    assert_eq!(
        agentverify_contract::load_file(&json_path)
            .unwrap()
            .action_name,
        "create_customer"
    );

    let yaml_path = dir.path().join("contract-yaml");
    std::fs::write(&yaml_path, VALID_YAML).unwrap();
    assert_eq!(
        agentverify_contract::load_file(&yaml_path)
            .unwrap()
            .action_name,
        "create_customer"
    );
}

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

#[test]
fn to_json_produces_reparseable_output() {
    let contract = Contract::new("create_customer")
        .with_postcondition(Predicate::exists("customer.id"), "Customer was created");
    let json = to_json(&contract).unwrap();
    let reparsed = parse_json(&json).unwrap();
    assert_eq!(reparsed.action_name, "create_customer");
    assert_eq!(reparsed.postconditions.len(), 1);
    assert_eq!(reparsed.id, contract.id);
}

#[test]
fn to_yaml_produces_reparseable_output() {
    let contract = Contract::new("create_customer")
        .with_postcondition(Predicate::exists("customer.id"), "Customer was created");
    let yaml = to_yaml(&contract).unwrap();
    let reparsed = parse_yaml(&yaml).unwrap();
    assert_eq!(reparsed.action_name, "create_customer");
    assert_eq!(reparsed.postconditions.len(), 1);
    assert_eq!(reparsed.id, contract.id);
}

#[test]
fn validate_accepts_a_complete_contract_with_nested_predicates() {
    let contract = Contract::new("create_customer")
        .with_precondition(
            Predicate::all(vec![
                Predicate::exists("customer.name"),
                Predicate::exists("customer.email"),
            ]),
            "Customer input present",
        )
        .with_postcondition(
            Predicate::any(vec![
                Predicate::exists("customer.id"),
                Predicate::negate(Predicate::exists("customer.pending")),
            ]),
            "Customer settled",
        );
    validate_contract(&contract).unwrap();
}

#[test]
fn validate_descends_into_implications() {
    // `implies` is validated recursively through both of its branches, and
    // the reported paths name the branch that failed.
    let err = parse_json(
        r#"{
        "action_name": "create_customer",
        "postconditions": [
            {
                "predicate": {
                    "type": "implies",
                    "antecedent": {"type": "exists", "path": "customer.id"},
                    "consequent": {"type": "all", "predicates": []}
                },
                "description": "settled"
            }
        ]
    }"#,
    )
    .unwrap_err();
    match &err {
        ContractError::InvalidPredicate { path, .. } => {
            assert_eq!(path.to_string(), "at postconditions[0].consequent");
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn invalid_predicate_with_an_empty_context_renders_without_a_context_section() {
    let err = ContractError::InvalidPredicate {
        reason: "All predicate must have at least one predicate".to_string(),
        path: PredicatePath {
            path: "postconditions[0]".to_string(),
        },
        context: Some(ContractContext {
            contract_id: None,
            action_name: None,
        }),
    };
    assert_eq!(
        err.to_string(),
        "Invalid predicate at postconditions[0]: \
         All predicate must have at least one predicate"
    );
}
