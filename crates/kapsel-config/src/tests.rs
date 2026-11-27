//! Ported from tinygo/json_bridge_test.go (TestValidateJsonConfig) plus the
//! decoding rules of Go's encoding/json that the component inherits.

use super::*;

fn op(t: &str) -> Operation {
    Operation {
        op_type: t.to_string(),
        ..Default::default()
    }
}

fn cfg(ws: &str, ops: Vec<Operation>) -> JsonConfig {
    JsonConfig {
        workspace_dir: ws.to_string(),
        operations: ops,
    }
}

#[test]
fn validate_json_config_table() {
    let mkdir = |p: &str| Operation {
        path: p.into(),
        ..op("mkdir")
    };
    let copy = |s: &str, d: &str| Operation {
        src_path: s.into(),
        dest_path: d.into(),
        ..op("copy_file")
    };
    let cases: Vec<(&str, JsonConfig, Option<&str>)> = vec![
        (
            "valid config",
            cfg(
                "/tmp/ws",
                vec![mkdir("include"), copy("/absolute/src", "relative/dest")],
            ),
            None,
        ),
        (
            "empty workspace dir",
            cfg("", vec![mkdir("test")]),
            Some("workspace_dir cannot be empty"),
        ),
        (
            "copy_file missing src_path",
            cfg(
                "/tmp/ws",
                vec![Operation {
                    dest_path: "dest".into(),
                    ..op("copy_file")
                }],
            ),
            Some("operation 0: copy_file requires src_path and dest_path"),
        ),
        (
            "copy_file absolute dest_path",
            cfg("/tmp/ws", vec![copy("/absolute/src", "/absolute/dest")]),
            Some("operation 0: dest_path must be relative: /absolute/dest"),
        ),
        (
            "mkdir absolute path",
            cfg("/tmp/ws", vec![mkdir("/absolute/path")]),
            Some("operation 0: mkdir path must be relative: /absolute/path"),
        ),
        (
            "unknown operation type",
            cfg("/tmp/ws", vec![op("unknown_operation")]),
            Some("operation 0: unknown operation type: unknown_operation"),
        ),
        (
            "relative source under an absolute workspace",
            cfg("/tmp/ws", vec![copy("src/a", "a")]),
            Some("operation 0: src_path must be absolute: src/a"),
        ),
    ];
    for (name, c, want) in cases {
        assert_eq!(validate(&c).err().as_deref(), want, "{name}");
    }
}

/// REQ-KS-017: a relative workspace_dir with execroot-relative paths is
/// accepted; TinyGo refused it ("workspace_dir must be an absolute path").
#[test]
fn relative_workspace_dir_is_accepted() {
    let c = cfg(
        ".",
        vec![
            Operation {
                src_path: "bazel-out/k8/bin/a.txt".into(),
                dest_path: "bazel-out/k8/bin/x/a.txt".into(),
                ..op("copy_file")
            },
            Operation {
                path: "bazel-out/k8/bin/x/inc".into(),
                ..op("mkdir")
            },
            Operation {
                sources: vec!["a.txt".into(), "/abs/b.txt".into()],
                dest_path: "out.txt".into(),
                ..op("concatenate_files")
            },
        ],
    );
    assert_eq!(validate(&c), Ok(()));
    let c = cfg(
        "relative/path",
        vec![Operation {
            path: "test".into(),
            ..op("mkdir")
        }],
    );
    assert_eq!(validate(&c), Ok(()));
}

/// REQ-KS-017: an unknown operation is an error, also in relative mode.
#[test]
fn unknown_operation_fails_in_relative_mode() {
    let c = cfg(".", vec![op("copy_file_typo")]);
    assert_eq!(
        validate(&c).unwrap_err(),
        "operation 0: unknown operation type: copy_file_typo"
    );
}

#[test]
fn every_operation_type_validates() {
    for t in OPERATION_TYPES {
        let o = Operation {
            op_type: t.into(),
            src_path: "/s".into(),
            dest_path: "d".into(),
            path: if t == "read_file" {
                "/p".into()
            } else {
                "p".into()
            },
            command: "c".into(),
            sources: vec!["/s".into()],
            ..Default::default()
        };
        assert_eq!(validate(&cfg("/ws", vec![o])), Ok(()), "{t}");
    }
}

#[test]
fn decoding_follows_go() {
    // Case-insensitive keys, unknown keys ignored, null leaves a field unset.
    let c = parse_json_config(
        br#"{"WORKSPACE_DIR":"/w","extra":{"x":[1]},"Operations":[{"TYPE":"mkdir","Path":"a","path":null}]}"#,
    )
    .unwrap();
    assert_eq!(c.workspace_dir, "/w");
    assert_eq!(
        c.operations,
        vec![Operation {
            path: "a".into(),
            ..op("mkdir")
        }]
    );

    // A repeated key wins last; a repeated array decodes into the old elements.
    let c = parse_json_config(
        br#"{"workspace_dir":"/a","workspace_dir":"/b","operations":[{"type":"mkdir","path":"x"},{"type":"t2"}],"operations":[{"path":"y"}]}"#,
    )
    .unwrap();
    assert_eq!(c.workspace_dir, "/b");
    assert_eq!(
        c.operations,
        vec![Operation {
            path: "y".into(),
            ..op("mkdir")
        }]
    );

    let c = parse_json_config(
        br#"{"workspace_dir":"/a","operations":[{"type":"a"},{"type":"b"}],"operations":[{"path":"1"}],"operations":[{},{}]}"#,
    )
    .unwrap();
    assert_eq!(
        c.operations[1].op_type, "b",
        "elements past len survive a shrink"
    );

    // null resets a slice.
    let c = parse_json_config(
        br#"{"workspace_dir":"/a","operations":[{"type":"a"}],"operations":null}"#,
    )
    .unwrap();
    assert!(c.operations.is_empty());
}

#[test]
fn type_errors_match_go() {
    let cases: &[(&[u8], &str)] = &[
        (
            br#"{"workspace_dir": 5}"#,
            "json: cannot unmarshal number into Go struct field JsonConfig.workspace_dir of type string",
        ),
        (
            br#"[1,2]"#,
            "json: cannot unmarshal array into Go value of type main.JsonConfig",
        ),
        (
            br#""x""#,
            "json: cannot unmarshal string into Go value of type main.JsonConfig",
        ),
        (
            br#"{"operations":{}}"#,
            "json: cannot unmarshal object into Go struct field JsonConfig.operations of type []main.Operation",
        ),
        (
            br#"{"operations":[1]}"#,
            "json: cannot unmarshal number into Go struct field JsonConfig.operations of type main.Operation",
        ),
        (
            br#"{"operations":[{"type":5}]}"#,
            "json: cannot unmarshal number into Go struct field Operation.operations.type of type string",
        ),
        (
            br#"{"operations":[{"args":"x"}]}"#,
            "json: cannot unmarshal string into Go struct field Operation.operations.args of type []string",
        ),
        (
            br#"{"operations":[{"Args":[true]}]}"#,
            "json: cannot unmarshal bool into Go struct field Operation.operations.args of type string",
        ),
        // The first type error wins.
        (
            br#"{"workspace_dir":[],"operations":5}"#,
            "json: cannot unmarshal array into Go struct field JsonConfig.workspace_dir of type string",
        ),
    ];
    for (input, want) in cases {
        assert_eq!(
            parse_json_config(input).unwrap_err(),
            *want,
            "{}",
            String::from_utf8_lossy(input)
        );
    }
    // Syntax errors win over type errors.
    assert_eq!(
        parse_json_config(br#"{"workspace_dir": 5,}"#).unwrap_err(),
        "invalid character '}' looking for beginning of object key string"
    );
    // Top-level null decodes to the zero value.
    assert_eq!(parse_json_config(b"null").unwrap(), JsonConfig::default());
}

#[test]
fn workspace_config_decodes() {
    let w = parse_workspace_config(
        br#"{"work_dir":"/w","sources":[{"source":"/a","destination":"b"}],"workspace_type":2,"security_config":{"level":1}}"#,
    )
    .unwrap();
    assert_eq!(w.work_dir, "/w");
    assert_eq!(w.sources[0].destination.as_deref(), Some("b"));
    assert_eq!(w.workspace_type, 2);
    assert_eq!(w.security_config.unwrap().level, 1);
    assert_eq!(
        parse_workspace_config(br#"{"workspace_type":1.5}"#).unwrap_err(),
        "json: cannot unmarshal number 1.5 into Go struct field WorkspaceConfig.workspace_type of type main.WorkspaceType"
    );
    assert_eq!(
        parse_workspace_config(br#"{"sources":[{"destination":3}]}"#).unwrap_err(),
        "json: cannot unmarshal number into Go struct field FileSpec.sources.destination of type string"
    );
}

#[test]
fn serde_model_round_trips() {
    let c = cfg(
        "/w",
        vec![Operation {
            path: "a".into(),
            ..op("mkdir")
        }],
    );
    let s = serde_json::to_string(&c).unwrap();
    assert_eq!(
        s,
        r#"{"workspace_dir":"/w","operations":[{"type":"mkdir","path":"a"}]}"#
    );
    assert_eq!(parse_json_config(s.as_bytes()).unwrap(), c);
}
