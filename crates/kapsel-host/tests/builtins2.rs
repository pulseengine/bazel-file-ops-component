//! REQ-KS-063 built-ins: golden stdout, stderr, status and file contents per
//! built-in and flag; comment preservation for YAML and TOML (byte-exact);
//! atomic writes; grants (DD-KS-020).

mod common;
use common::T;

use std::io::Cursor;
use std::os::unix::fs::symlink;

use kapsel_host::{GrantSet, Session, SharedBuf};

// ---- edit --------------------------------------------------------------------

#[test]
fn edit_literal() {
    let mut t = T::new();
    t.write("f.txt", "a.b a.b\nc\n");
    t.check(&["edit", "f.txt", "-s", "a.b", "X"], "", 0, "", "");
    assert_eq!(t.read("f.txt"), "X X\nc\n");
    // Literal: the dot is not a regex.
    t.write("g.txt", "axb\n");
    t.check(
        &["edit", "g.txt", "-s", "a.b", "X"],
        "",
        1,
        "",
        "edit: g.txt: 'a.b' not found; file unchanged (--allow-none accepts no match)\n",
    );
    assert_eq!(t.read("g.txt"), "axb\n");
    t.check(
        &["edit", "g.txt", "-s", "a.b", "X", "--allow-none"],
        "",
        0,
        "",
        "",
    );
    // Options may come first; an OLD that looks like a flag is taken as text.
    t.write("h.txt", "--x--\n");
    t.check(&["edit", "-s", "--x", "-y", "h.txt"], "", 0, "", "");
    assert_eq!(t.read("h.txt"), "-y--\n");
}

#[test]
fn edit_regex_groups_and_lines() {
    let mut t = T::new();
    t.write("v.toml", "version = \"0.1.0\"\nname = \"x\"\n");
    t.check(
        &[
            "edit",
            "v.toml",
            "-e",
            r#"^version = "(\d+)\.(\d+)\.\d+"$"#,
            r#"version = "$1.${2}.9""#,
        ],
        "",
        0,
        "",
        "",
    );
    assert_eq!(t.read("v.toml"), "version = \"0.1.9\"\nname = \"x\"\n");
    // Without -m, `.` stops at a newline; with it, it crosses lines.
    t.write("m.txt", "start\nmiddle\nend\n");
    t.check(
        &["edit", "m.txt", "-e", "start.*end", "X"],
        "",
        1,
        "",
        "edit: m.txt: /start.*end/ not found; file unchanged (--allow-none accepts no match)\n",
    );
    t.check(
        &["edit", "m.txt", "-e", "start.*end", "X", "-m"],
        "",
        0,
        "",
        "",
    );
    assert_eq!(t.read("m.txt"), "X\n");
    // An explicit \n matches across lines without -m.
    t.write("n.txt", "a\nb\n");
    t.check(&["edit", "n.txt", "-e", r"a\nb", "ab"], "", 0, "", "");
    assert_eq!(t.read("n.txt"), "ab\n");
    t.check(
        &["edit", "n.txt", "-e", "(", "x"],
        "",
        2,
        "",
        "edit: invalid regex '(': error: unclosed group\n",
    );
}

#[test]
fn edit_count_is_atomic() {
    let mut t = T::new();
    t.write("c.txt", "x x x\n");
    t.check(
        &["edit", "c.txt", "-s", "x", "y", "-c", "2"],
        "",
        1,
        "",
        "edit: c.txt: 'x' matched 3 times, -c requires 2; file unchanged\n",
    );
    // No partial write and no temporary file left behind.
    assert_eq!(t.read("c.txt"), "x x x\n");
    let names: Vec<String> = std::fs::read_dir(t.root())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["c.txt"]);
    t.check(&["edit", "c.txt", "-e", "x", "y", "-c", "3"], "", 0, "", "");
    assert_eq!(t.read("c.txt"), "y y y\n");
}

#[test]
fn edit_keeps_mode() {
    use std::os::unix::fs::PermissionsExt;
    let mut t = T::new();
    t.write("run.sh", "echo a\n");
    std::fs::set_permissions(t.path("run.sh"), std::fs::Permissions::from_mode(0o751)).unwrap();
    t.check(&["edit", "run.sh", "-s", "a", "b"], "", 0, "", "");
    let mode = std::fs::metadata(t.path("run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o751);
}

#[test]
fn edit_splice_lines() {
    let mut t = T::new();
    t.write("s.txt", "1\n2\n3\n4\n");
    t.write("new.txt", "two\nthree");
    t.check(
        &["edit", "s.txt", "--lines", "2:3", "--with", "new.txt"],
        "",
        0,
        "",
        "",
    );
    assert_eq!(t.read("s.txt"), "1\ntwo\nthree\n4\n");
    t.check(
        &["edit", "s.txt", "--lines", "$:$", "--with-text", "end"],
        "",
        0,
        "",
        "",
    );
    assert_eq!(t.read("s.txt"), "1\ntwo\nthree\nend\n");
    // A:A-1 inserts before line A.
    t.check(
        &["edit", "s.txt", "--lines", "1:0", "--with-text", "zero"],
        "",
        0,
        "",
        "",
    );
    assert_eq!(t.read("s.txt"), "zero\n1\ntwo\nthree\nend\n");
    // Empty text deletes.
    t.check(
        &["edit", "s.txt", "--lines", "2:3", "--with-text", ""],
        "",
        0,
        "",
        "",
    );
    assert_eq!(t.read("s.txt"), "zero\nthree\nend\n");
    t.check(
        &["edit", "s.txt", "--lines", "3:9", "--with-text", "x"],
        "",
        1,
        "",
        "edit: s.txt: range 3:9 is outside the file's 3 lines; file unchanged\n",
    );
    assert_eq!(t.read("s.txt"), "zero\nthree\nend\n");
}

#[test]
fn edit_usage_errors() {
    let mut t = T::new();
    t.write("f", "x\n");
    let usage = "usage: edit FILE (-s OLD NEW | -e REGEX REPL [-m] | --lines A:B (--with FILE2 | --with-text TEXT)) [-c N] [--allow-none]";
    t.check(
        &["edit", "f"],
        "",
        2,
        "",
        &format!("edit: give exactly one of -s, -e or --lines, once; {usage}\n"),
    );
    t.check(
        &["edit", "f", "-s", "x", "y", "-e", "x", "y"],
        "",
        2,
        "",
        &format!("edit: give exactly one of -s, -e or --lines, once; {usage}\n"),
    );
    t.check(
        &["edit", "f", "-m", "-s", "x", "y"],
        "",
        2,
        "",
        &format!("edit: -m applies to -e; {usage}\n"),
    );
    t.check(
        &["edit", "f", "-s", "x"],
        "",
        2,
        "",
        "edit: option '-s' requires 2 arguments\n",
    );
    t.check(
        &["edit", "f", "-i", "-s", "x", "y"],
        "",
        2,
        "",
        "edit: unknown flag -i; kapsel's edit supports: -s X Y -e X Y -m -c X --allow-none --lines X --with X --with-text X\n",
    );
    t.check(
        &["edit", "missing", "-s", "x", "y"],
        "",
        1,
        "",
        "edit: missing: No such file or directory\n",
    );
}

// ---- json --------------------------------------------------------------------

const DOC: &str = "{\n  \"name\": \"kapsel\",\n  \"tags\": [\"a\", \"b\"],\n  \"deps\": {\"x y\": 1, \"z\": null}\n}\n";

#[test]
fn json_get_paths_and_flags() {
    let mut t = T::new();
    t.write("d.json", DOC);
    t.check(
        &["json", "get", ".name", "d.json"],
        "",
        0,
        "\"kapsel\"\n",
        "",
    );
    t.check(
        &["json", "get", "-r", ".name", "d.json"],
        "",
        0,
        "kapsel\n",
        "",
    );
    t.check(&["json", "get", ".tags[1]", "d.json"], "", 0, "\"b\"\n", "");
    t.check(
        &["json", "get", ".tags[-1]", "d.json"],
        "",
        0,
        "\"b\"\n",
        "",
    );
    t.check(
        &["json", "get", "-r", ".tags[]", "d.json"],
        "",
        0,
        "a\nb\n",
        "",
    );
    t.check(
        &["json", "get", ".deps.\"x y\"", "d.json"],
        "",
        0,
        "1\n",
        "",
    );
    t.check(
        &["json", "get", ".deps[\"x y\"]", "d.json"],
        "",
        0,
        "1\n",
        "",
    );
    t.check(
        &["json", "get", ".deps", "d.json"],
        "",
        0,
        "{\n  \"x y\": 1,\n  \"z\": null\n}\n",
        "",
    );
    t.check(
        &["json", "get", "-c", ".deps", "d.json"],
        "",
        0,
        "{\"x y\":1,\"z\":null}\n",
        "",
    );
    t.check(
        &["json", "get", "-cr", ".", "d.json"],
        "",
        0,
        "{\"name\":\"kapsel\",\"tags\":[\"a\",\"b\"],\"deps\":{\"x y\":1,\"z\":null}}\n",
        "",
    );
    // stdin when FILE is omitted.
    t.check(&["json", "get", ".a"], "{\"a\": [1]}", 0, "[\n  1\n]\n", "");
    // A missing key is an error, never a silent null.
    t.check(
        &["json", "get", ".deps.q", "d.json"],
        "",
        1,
        "",
        "json: .deps.q: no such key\n",
    );
    t.check(
        &["json", "get", ".name[0]", "d.json"],
        "",
        1,
        "",
        "json: .name: a string cannot be indexed with [0]\n",
    );
    t.check(
        &["json", "get", "name", "d.json"],
        "",
        2,
        "",
        "json: bad path 'name': a path starts with `.` (paths look like .a.b, .a[0], .\"a b\")\n",
    );
    t.check(
        &["json", "get", ".a"],
        "{",
        1,
        "",
        "json: -: invalid JSON: expected a key at line 1 column 2\n",
    );
    t.check(
        &["json", "get", ".a"],
        "{\"a\": 01}",
        1,
        "",
        "json: -: invalid JSON: expected `,` or `}` at line 1 column 8\n",
    );
}

/// Numbers are printed as written, never through a float.
#[test]
fn json_get_keeps_number_text() {
    let mut t = T::new();
    t.write("n.json", NUMS_JSON);
    for (path, out) in [
        (".version", "1.10\n"),
        (".ratio", "0.1000\n"),
        (".huge", "123456789012345678901234567890\n"),
        (".big", "1e400\n"),
        (".negzero", "-0\n"),
    ] {
        t.check(&["json", "get", path, "n.json"], "", 0, out, "");
    }
    t.check(
        &["json", "get", "-c", ".", "n.json"],
        "",
        0,
        "{\"version\":1.10,\"ratio\":0.1000,\"huge\":123456789012345678901234567890,\"big\":1e400,\"negzero\":-0,\"n\":1}\n",
        "",
    );
}

const NUMS_JSON: &str = "{\"version\": 1.10, \"ratio\": 0.1000, \"huge\": 123456789012345678901234567890, \"big\": 1e400, \"negzero\": -0, \"n\": 1}\n";

/// The coordinator's reproduction: `json set .n 2` changes `1` to `2` and
/// not one other byte.
#[test]
fn json_set_changes_only_the_node() {
    let mut t = T::new();
    let src = "{\"version\": 1.10, \"ratio\": 0.1000, \"huge\": 123456789012345678901234567890, \"n\": 1}\n";
    t.write("f.json", src);
    t.check(&["json", "set", ".n", "2", "f.json"], "", 0, "", "");
    assert_eq!(t.read("f.json"), src.replace("\"n\": 1", "\"n\": 2"));
    // The same with every unusual number, across set, insert and del.
    t.write("n.json", NUMS_JSON);
    t.check(&["json", "set", ".n", "2", "n.json"], "", 0, "", "");
    assert_eq!(t.read("n.json"), NUMS_JSON.replace("\"n\": 1", "\"n\": 2"));
    t.check(&["json", "set", ".x", "1.50", "n.json"], "", 0, "", "");
    assert_eq!(
        t.read("n.json"),
        NUMS_JSON.replace("\"n\": 1}", "\"n\": 2, \"x\": 1.50}")
    );
    t.check(&["json", "del", ".x", "n.json"], "", 0, "", "");
    t.check(&["json", "del", ".n", "n.json"], "", 0, "", "");
    assert_eq!(t.read("n.json"), NUMS_JSON.replace(", \"n\": 1", ""));
    t.check(&["json", "del", ".version", "n.json"], "", 0, "", "");
    assert_eq!(
        t.read("n.json"),
        "{\"ratio\": 0.1000, \"huge\": 123456789012345678901234567890, \"big\": 1e400, \"negzero\": -0}\n"
    );
}

#[test]
fn json_del_first_middle_last() {
    let mut t = T::new();
    let src = "{\n  \"a\": 1,\n  \"b\": [1, 2, 3],\n  \"c\": {\"x\": 1.10}\n}\n";
    for (path, want) in [
        (".a", "{\n  \"b\": [1, 2, 3],\n  \"c\": {\"x\": 1.10}\n}\n"),
        (".b", "{\n  \"a\": 1,\n  \"c\": {\"x\": 1.10}\n}\n"),
        (".c", "{\n  \"a\": 1,\n  \"b\": [1, 2, 3]\n}\n"),
        (
            ".b[0]",
            "{\n  \"a\": 1,\n  \"b\": [2, 3],\n  \"c\": {\"x\": 1.10}\n}\n",
        ),
        (
            ".b[1]",
            "{\n  \"a\": 1,\n  \"b\": [1, 3],\n  \"c\": {\"x\": 1.10}\n}\n",
        ),
        (
            ".b[-1]",
            "{\n  \"a\": 1,\n  \"b\": [1, 2],\n  \"c\": {\"x\": 1.10}\n}\n",
        ),
        (
            ".c.x",
            "{\n  \"a\": 1,\n  \"b\": [1, 2, 3],\n  \"c\": {}\n}\n",
        ),
    ] {
        t.write("d.json", src);
        t.check(&["json", "del", path, "d.json"], "", 0, "", "");
        assert_eq!(t.read("d.json"), want, "del {path}");
    }
}

#[test]
fn json_insert_and_nested_index() {
    let mut t = T::new();
    // Into an empty object, multi-line file: the siblings' indent.
    t.write("e.json", "{\n  \"deps\": {},\n  \"v\": 1.10\n}\n");
    t.check(&["json", "set", ".deps.a", "1", "e.json"], "", 0, "", "");
    assert_eq!(
        t.read("e.json"),
        "{\n  \"deps\": {\n    \"a\": 1\n  },\n  \"v\": 1.10\n}\n"
    );
    // Into an empty object, one-line file; and an empty document object.
    t.write("o.json", "{}");
    t.check(&["json", "set", ".a", "[1, 2]", "o.json"], "", 0, "", "");
    assert_eq!(t.read("o.json"), "{\"a\":[1,2]}");
    // A nested array index, and an append at the length.
    t.write("a.json", "{\"m\": [[1, 2], [3, 1e400]]}\n");
    t.check(&["json", "set", ".m[1][0]", "-0", "a.json"], "", 0, "", "");
    t.check(&["json", "set", ".m[0][2]", "9", "a.json"], "", 0, "", "");
    assert_eq!(t.read("a.json"), "{\"m\": [[1, 2, 9], [-0, 1e400]]}\n");
    // A new key with a nested value takes the file's indent below the sibling.
    t.write("n.json", "{\n    \"a\": 1\n}\n");
    t.check(
        &["json", "set", ".b.c", "{\"d\": 0.50}", "n.json"],
        "",
        0,
        "",
        "",
    );
    assert_eq!(
        t.read("n.json"),
        "{\n    \"a\": 1,\n    \"b\": {\n        \"c\": {\n            \"d\": 0.50\n        }\n    }\n}\n"
    );
    // A tab-indented file and a null that becomes an object.
    t.write("t.json", "{\n\t\"a\": null\n}\n");
    t.check(&["json", "set", ".a.b", "true", "t.json"], "", 0, "", "");
    assert_eq!(t.read("t.json"), "{\n\t\"a\": {\"b\": true}\n}\n");
}

#[test]
fn json_set_keeps_order_and_layout() {
    let mut t = T::new();
    t.write("d.json", DOC);
    t.check(
        &["json", "set", ".name", "\"shell\"", "d.json"],
        "",
        0,
        "",
        "",
    );
    t.check(
        &["json", "set", ".deps.z", "--str", "2.0", "d.json"],
        "",
        0,
        "",
        "",
    );
    t.check(
        &["json", "set", ".tags[2]", "\"c\"", "d.json"],
        "",
        0,
        "",
        "",
    );
    t.check(&["json", "set", ".new.deep", "-1", "d.json"], "", 0, "", "");
    assert_eq!(
        t.read("d.json"),
        "{\n  \"name\": \"shell\",\n  \"tags\": [\"a\", \"b\", \"c\"],\n  \"deps\": {\"x y\": 1, \"z\": \"2.0\"},\n  \"new\": {\n    \"deep\": -1\n  }\n}\n"
    );
    // A one-line file stays one line; four-space indent stays four.
    t.write("c.json", "{\"b\":1,\"a\":2}");
    t.check(&["json", "set", ".b", "3", "c.json"], "", 0, "", "");
    assert_eq!(t.read("c.json"), "{\"b\":3,\"a\":2}");
    t.write("w.json", "{\n    \"b\": 1\n}\n");
    t.check(&["json", "set", ".a", "[1]", "w.json"], "", 0, "", "");
    assert_eq!(
        t.read("w.json"),
        "{\n    \"b\": 1,\n    \"a\": [\n        1\n    ]\n}\n"
    );
    t.check(
        &["json", "set", ".b", "shell", "w.json"],
        "",
        2,
        "",
        "json: VALUE 'shell' is not JSON (expected a value at line 1 column 1); use --str to set it as a string\n",
    );
    t.check(
        &["json", "set", ".b[5]", "1", "w.json"],
        "",
        1,
        "",
        "json: w.json: .b: a number cannot be indexed with [5]; file unchanged\n",
    );
}

#[test]
fn json_del_and_merge() {
    let mut t = T::new();
    t.write("d.json", DOC);
    t.check(&["json", "del", ".tags[0]", "d.json"], "", 0, "", "");
    t.check(&["json", "del", ".deps.\"x y\"", "d.json"], "", 0, "", "");
    assert_eq!(
        t.read("d.json"),
        "{\n  \"name\": \"kapsel\",\n  \"tags\": [\"b\"],\n  \"deps\": {\"z\": null}\n}\n"
    );
    t.check(
        &["json", "del", ".nope", "d.json"],
        "",
        1,
        "",
        "json: d.json: .nope: no such key; file unchanged\n",
    );
    // merge rewrites the layout (its usage says so) but keeps number text.
    t.write(
        "o.json",
        "{\"deps\": {\"w\": 1.10}, \"name\": \"n\", \"big\": 1e400}",
    );
    t.check(&["json", "merge", "d.json", "o.json"], "", 0, "", "");
    assert_eq!(
        t.read("d.json"),
        "{\n  \"name\": \"n\",\n  \"tags\": [\n    \"b\"\n  ],\n  \"deps\": {\n    \"z\": null,\n    \"w\": 1.10\n  },\n  \"big\": 1e400\n}\n"
    );
    t.check(
        &["json", "frob", ".a", "d.json"],
        "",
        2,
        "",
        "json: unknown subcommand 'frob'; usage: json get PATH [FILE] | json set PATH VALUE FILE | json del PATH FILE | json merge FILE OTHER (rewrites FILE in a normal layout)\n",
    );
    t.check(
        &["json", "set", ".a[]", "1", "d.json"],
        "",
        2,
        "",
        "json: `[]` selects many values; set and del take one path\n",
    );
}

// ---- yaml --------------------------------------------------------------------

const CONFIG: &str = "\
# Service configuration (do not reorder)
name: kapsel   # the shell
version: 1
tags:
  - a     # first
  - 'b'
owner:
  team: shell
  # who to page
  pager: \"ops\"
script: |
  echo one
  echo two
last: z # end
";

#[test]
fn yaml_get() {
    let mut t = T::new();
    t.write("c.yaml", CONFIG);
    t.check(&["yaml", "get", ".name", "c.yaml"], "", 0, "kapsel\n", "");
    t.check(&["yaml", "get", ".tags[1]", "c.yaml"], "", 0, "b\n", "");
    t.check(&["yaml", "get", ".tags[]", "c.yaml"], "", 0, "a\nb\n", "");
    t.check(&["yaml", "get", ".version", "c.yaml"], "", 0, "1\n", "");
    t.check(
        &["yaml", "get", ".owner", "c.yaml"],
        "",
        0,
        "team: shell\npager: ops\n",
        "",
    );
    t.check(
        &["yaml", "get", "-j", ".owner", "c.yaml"],
        "",
        0,
        "{\n  \"team\": \"shell\",\n  \"pager\": \"ops\"\n}\n",
        "",
    );
    t.check(
        &["yaml", "get", ".script", "c.yaml"],
        "",
        0,
        "echo one\necho two\n\n",
        "",
    );
    t.check(
        &["yaml", "get", ".a.b"],
        "a:\n  b: [1, 2]\n",
        0,
        "- 1\n- 2\n",
        "",
    );
    t.check(
        &["yaml", "get", ".owner.email", "c.yaml"],
        "",
        1,
        "",
        "yaml: .owner.email: no such key\n",
    );
}

/// Byte-exact golden of an edited file with comments: everything outside
/// the edited node is unchanged.
#[test]
fn yaml_set_preserves_comments() {
    let mut t = T::new();
    t.write("c.yaml", CONFIG);
    t.check(&["yaml", "set", ".version", "2", "c.yaml"], "", 0, "", "");
    t.check(
        &["yaml", "set", ".owner.pager", "on-call", "c.yaml"],
        "",
        0,
        "",
        "",
    );
    t.check(
        &["yaml", "set", ".tags[0]", "--str", "yes", "c.yaml"],
        "",
        0,
        "",
        "",
    );
    t.check(&["yaml", "set", ".tags[2]", "c", "c.yaml"], "", 0, "", "");
    t.check(
        &["yaml", "set", ".owner.slack", "#shell", "c.yaml"],
        "",
        2,
        "",
        "yaml: VALUE '#shell' reads as null in YAML; use --str to set it as a string, or write null\n",
    );
    t.check(
        &["yaml", "set", ".owner.slack", "--str", "#shell", "c.yaml"],
        "",
        0,
        "",
        "",
    );
    t.check(
        &["yaml", "set", ".added.deep", "[1, x]", "c.yaml"],
        "",
        0,
        "",
        "",
    );
    assert_eq!(
        t.read("c.yaml"),
        "\
# Service configuration (do not reorder)
name: kapsel   # the shell
version: 2
tags:
  - \"yes\"     # first
  - 'b'
  - c
owner:
  team: shell
  # who to page
  pager: on-call
  slack: \"#shell\"
script: |
  echo one
  echo two
last: z # end
added:
  deep: [1, x]
"
    );
}

#[test]
fn yaml_set_replaces_a_block_node() {
    let mut t = T::new();
    t.write("c.yaml", CONFIG);
    t.check(
        &["yaml", "set", ".owner", "{team: x}", "c.yaml"],
        "",
        0,
        "",
        "",
    );
    t.check(
        &["yaml", "set", ".script", "echo three", "c.yaml"],
        "",
        0,
        "",
        "",
    );
    assert_eq!(
        t.read("c.yaml"),
        "\
# Service configuration (do not reorder)
name: kapsel   # the shell
version: 1
tags:
  - a     # first
  - 'b'
owner: {team: x}
script: echo three
last: z # end
"
    );
}

#[test]
fn yaml_del_preserves_comments() {
    let mut t = T::new();
    t.write("c.yaml", CONFIG);
    t.check(&["yaml", "del", ".owner.team", "c.yaml"], "", 0, "", "");
    t.check(&["yaml", "del", ".tags[0]", "c.yaml"], "", 0, "", "");
    t.check(&["yaml", "del", ".version", "c.yaml"], "", 0, "", "");
    assert_eq!(
        t.read("c.yaml"),
        "\
# Service configuration (do not reorder)
name: kapsel   # the shell
tags:
  - 'b'
owner:
  # who to page
  pager: \"ops\"
script: |
  echo one
  echo two
last: z # end
"
    );
    // The last member leaves an empty collection, not a dangling key.
    t.check(&["yaml", "del", ".tags[0]", "c.yaml"], "", 0, "", "");
    assert!(
        t.read("c.yaml").contains("tags: []\n"),
        "{}",
        t.read("c.yaml")
    );
    t.check(
        &["yaml", "del", ".nope", "c.yaml"],
        "",
        1,
        "",
        "yaml: c.yaml: .nope: no such key; file unchanged\n",
    );
}

#[test]
fn yaml_refuses_what_it_cannot_splice() {
    let mut t = T::new();
    let src = "a: {x: 1}\nb: &anchor 1\nc: *anchor\n";
    t.write("f.yaml", src);
    t.check(
        &["yaml", "set", ".a.x", "2", "f.yaml"],
        "",
        1,
        "",
        "yaml: f.yaml: .a is a flow collection ({x: 1}); set the whole value at .a instead; file unchanged\n",
    );
    t.check(
        &["yaml", "set", ".b", "2", "f.yaml"],
        "",
        1,
        "",
        "yaml: f.yaml: .b: anchors and aliases are not edited; edit the anchored text with `edit`; file unchanged\n",
    );
    t.check(&["yaml", "set", ".a", "{x: 2}", "f.yaml"], "", 0, "", "");
    assert_eq!(t.read("f.yaml"), "a: {x: 2}\nb: &anchor 1\nc: *anchor\n");
    t.check(
        &["yaml", "set", ".a.x", "2", "missing.yaml"],
        "",
        1,
        "",
        "yaml: missing.yaml: No such file or directory\n",
    );
}

#[test]
fn yaml_set_in_sequence_of_mappings() {
    let mut t = T::new();
    t.write(
        "w.yml",
        "jobs:\n  - name: build # step 1\n    run: make\n  - name: test\n    run: make test\n",
    );
    t.check(
        &["yaml", "set", ".jobs[1].run", "cargo test", "w.yml"],
        "",
        0,
        "",
        "",
    );
    t.check(
        &["yaml", "set", ".jobs[0].env", "{A: 1}", "w.yml"],
        "",
        0,
        "",
        "",
    );
    t.check(&["yaml", "del", ".jobs[0].name", "w.yml"], "", 0, "", "");
    assert_eq!(
        t.read("w.yml"),
        "jobs:\n  - run: make\n    env:\n      A: 1\n  - name: test\n    run: cargo test\n"
    );
}

// ---- toml --------------------------------------------------------------------

const CARGO: &str = "\
# Workspace manifest
[package]
name = \"kapsel\"   # crate name
version = \"0.1.0\" # bumped by release

# dependencies below
[dependencies]
serde = { version = \"1\", features = [\"derive\"] } # pinned
list = [1, 2]
";

#[test]
fn toml_get() {
    let mut t = T::new();
    t.write("Cargo.toml", CARGO);
    t.check(
        &["toml", "get", ".package.name", "Cargo.toml"],
        "",
        0,
        "kapsel\n",
        "",
    );
    t.check(
        &["toml", "get", ".dependencies.serde.version", "Cargo.toml"],
        "",
        0,
        "1\n",
        "",
    );
    t.check(
        &["toml", "get", ".dependencies.list[1]", "Cargo.toml"],
        "",
        0,
        "2\n",
        "",
    );
    t.check(
        &["toml", "get", ".dependencies.list[]", "Cargo.toml"],
        "",
        0,
        "1\n2\n",
        "",
    );
    t.check(
        &["toml", "get", ".dependencies.serde", "Cargo.toml"],
        "",
        0,
        "{ version = \"1\", features = [\"derive\"] }\n",
        "",
    );
    t.check(&["toml", "get", ".a.b"], "[a]\nb = true\n", 0, "true\n", "");
    t.check(
        &["toml", "get", ".package.edition", "Cargo.toml"],
        "",
        1,
        "",
        "toml: .package.edition: no such key\n",
    );
}

#[test]
fn toml_set_preserves_comments() {
    let mut t = T::new();
    t.write("Cargo.toml", CARGO);
    t.check(
        &["toml", "set", ".package.version", "\"0.2.0\"", "Cargo.toml"],
        "",
        0,
        "",
        "",
    );
    t.check(
        &[
            "toml",
            "set",
            ".package.name",
            "--str",
            "shell",
            "Cargo.toml",
        ],
        "",
        0,
        "",
        "",
    );
    t.check(
        &[
            "toml",
            "set",
            ".dependencies.serde.version",
            "\"1.0.200\"",
            "Cargo.toml",
        ],
        "",
        0,
        "",
        "",
    );
    t.check(
        &["toml", "set", ".dependencies.list[2]", "3", "Cargo.toml"],
        "",
        0,
        "",
        "",
    );
    t.check(
        &["toml", "set", ".dependencies.regex", "\"1\"", "Cargo.toml"],
        "",
        0,
        "",
        "",
    );
    assert_eq!(
        t.read("Cargo.toml"),
        "\
# Workspace manifest
[package]
name = \"shell\"   # crate name
version = \"0.2.0\" # bumped by release

# dependencies below
[dependencies]
serde = { version = \"1.0.200\", features = [\"derive\"] } # pinned
list = [1, 2, 3]
regex = \"1\"
"
    );
    t.check(
        &["toml", "set", ".package.name", "shell", "Cargo.toml"],
        "",
        2,
        "",
        "toml: VALUE 'shell' is not a TOML value; use --str to set it as a string\n",
    );
    t.check(
        &["toml", "del", ".package.name", "Cargo.toml"],
        "",
        2,
        "",
        "toml: unknown subcommand 'del'; usage: toml get PATH [FILE] | toml set PATH VALUE FILE\n",
    );
}

// ---- field, lines ------------------------------------------------------------

#[test]
fn field_cases() {
    let mut t = T::new();
    t.write("ps.txt", "  PID TTY   CMD\n  12 s001  zsh -l\n");
    t.check(&["field", "1", "ps.txt"], "", 0, "PID\n12\n", "");
    t.check(&["field", "3,1", "ps.txt"], "", 0, "CMD PID\nzsh 12\n", "");
    t.check(&["field", "NF", "ps.txt"], "", 0, "CMD\n-l\n", "");
    t.check(&["field", "-1", "ps.txt"], "", 0, "CMD\n-l\n", "");
    t.check(&["field", "-2", "ps.txt"], "", 0, "TTY\nzsh\n", "");
    t.check(&["field", "9", "ps.txt"], "", 0, "\n\n", "");
    t.check(&["field", "0"], "a  b\n", 0, "a  b\n", "");
    t.check(&["field", "2", "-d", ","], "a,,c\nx,y\n", 0, "\ny\n", "");
    t.check(
        &["field", "1,3", "-d", ":", "-o", ":"],
        "r:x:0:0\n",
        0,
        "r:0\n",
        "",
    );
    t.check(&["field", "2", "-d", "::"], "a::b::c\n", 0, "b\n", "");
    t.check(&["field", "1", "-d", " "], " a b\n", 0, "a\n", "");
    t.check(
        &["field", "x"],
        "",
        2,
        "",
        "field: 'x' is not a field number (N, -N or NF)\n",
    );
    t.check(
        &["field"],
        "",
        2,
        "",
        "field: missing field list; usage: field N[,M...] [-d DELIM] [-o SEP] [FILE...]\n",
    );
}

#[test]
fn lines_cases() {
    let mut t = T::new();
    t.write("n.txt", "1\n2\n3\n4\n5\n6\n");
    t.check(&["lines", "2:4", "n.txt"], "", 0, "2\n3\n4\n", "");
    t.check(&["lines", "5:$", "n.txt"], "", 0, "5\n6\n", "");
    t.check(&["lines", "5:", "n.txt"], "", 0, "5\n6\n", "");
    t.check(&["lines", "$", "n.txt"], "", 0, "6\n", "");
    t.check(&["lines", "3", "n.txt"], "", 0, "3\n", "");
    t.check(&["lines", "4:99", "n.txt"], "", 0, "4\n5\n6\n", "");
    t.check(&["lines", "9:12", "n.txt"], "", 0, "", "");
    t.check(&["lines", "1:2"], "a\nb\nc", 0, "a\nb\n", "");
    t.write("ci.yml", "a\nBEGIN\nx\nEND\nb\nBEGIN\ny\nEND\nc\n");
    t.check(
        &["lines", "/BEGIN/,/END/", "ci.yml"],
        "",
        0,
        "BEGIN\nx\nEND\nBEGIN\ny\nEND\n",
        "",
    );
    t.check(
        &["lines", "/^b$/,/^c$/", "ci.yml"],
        "",
        0,
        "b\nBEGIN\ny\nEND\nc\n",
        "",
    );
    t.check(
        &["lines", "0:2", "n.txt"],
        "",
        2,
        "",
        "lines: line numbers start at 1\n",
    );
    t.check(
        &["lines", "/a/", "n.txt"],
        "",
        2,
        "",
        "lines: '/a/': expected /RE1/,/RE2/\n",
    );
}

// ---- base64, sha256sum -------------------------------------------------------

#[test]
fn base64_cases() {
    let mut t = T::new();
    t.check(&["base64"], "hello", 0, "aGVsbG8=\n", "");
    t.check(&["base64"], "hi\n", 0, "aGkK\n", "");
    t.check(&["base64"], "", 0, "", "");
    t.check(&["base64", "-d"], "aGVsbG8=\n", 0, "hello", "");
    t.check(&["base64", "--decode"], "aGkK", 0, "hi\n", "");
    t.check(&["base64", "-d"], "a$b", 1, "", "base64: invalid input\n");
    let long = "x".repeat(60);
    let enc = "eHh4".repeat(20);
    t.check(
        &["base64"],
        &long,
        0,
        &format!("{}\n{}\n", &enc[..76], &enc[76..]),
        "",
    );
    t.check(&["base64", "-w", "0"], &long, 0, &format!("{enc}\n"), "");
    t.check(&["base64", "-w", "8"], "hello", 0, "aGVsbG8=\n", "");
    t.write("bin", "\u{0}\u{1}\u{2}");
    t.check(&["base64", "bin"], "", 0, "AAEC\n", "");
}

#[test]
fn sha256sum_cases() {
    let mut t = T::new();
    t.write("a", "abc");
    t.write("e", "");
    let a = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let e = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    t.check(
        &["sha256sum", "a", "e"],
        "",
        0,
        &format!("{a}  a\n{e}  e\n"),
        "",
    );
    t.check(&["sha256sum"], "abc", 0, &format!("{a}  -\n"), "");
    t.write("sums", &format!("{a}  a\n{e} *e\n"));
    t.check(&["sha256sum", "-c", "sums"], "", 0, "a: OK\ne: OK\n", "");
    t.write("a", "abd");
    t.write("bad", &format!("{a}  a\n{e}  gone\nnot a line\n"));
    t.check(
        &["sha256sum", "--check", "bad"],
        "",
        1,
        "a: FAILED\ngone: FAILED open or read\n",
        "sha256sum: gone: No such file or directory\n\
         sha256sum: WARNING: 1 line is improperly formatted\n\
         sha256sum: WARNING: 1 listed file could not be read\n\
         sha256sum: WARNING: 1 computed checksum did NOT match\n",
    );
    t.check(
        &["sha256sum", "nope"],
        "",
        1,
        "",
        "sha256sum: nope: No such file or directory\n",
    );
}

// ---- grants ------------------------------------------------------------------

/// <tmp>/work is read-write (cwd), <tmp>/ro read-only, <tmp>/out has no grant.
struct G {
    s: Session,
    base: tempfile::TempDir,
    out: SharedBuf,
    err: SharedBuf,
}

impl G {
    fn new() -> G {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().canonicalize().unwrap();
        for d in ["work", "ro", "out"] {
            std::fs::create_dir(root.join(d)).unwrap();
        }
        let grants = GrantSet {
            read_write: vec![root.join("work")],
            read_only: vec![root.join("ro")],
        };
        let s = Session::with_env(grants, root.join("work"), vec![]).unwrap();
        G {
            s,
            base,
            out: SharedBuf::new(),
            err: SharedBuf::new(),
        }
    }
    fn abs(&self, rel: &str) -> String {
        self.base
            .path()
            .canonicalize()
            .unwrap()
            .join(rel)
            .display()
            .to_string()
    }
    fn put(&self, rel: &str, text: &str) {
        std::fs::write(self.abs(rel), text).unwrap();
    }
    fn get(&self, rel: &str) -> String {
        std::fs::read_to_string(self.abs(rel)).unwrap()
    }
    fn run(&mut self, argv: &[&str]) -> (u32, String, String) {
        self.out.clear();
        self.err.clear();
        self.s.set_io(
            Box::new(Cursor::new(Vec::new())),
            self.out.clone(),
            self.err.clone(),
        );
        let st = self.s.call_direct(argv);
        (st, self.out.string(), self.err.string())
    }
}

/// Each writer, as (argv with FILE as `{}`, the file's starting content).
fn writers() -> Vec<(Vec<&'static str>, &'static str)> {
    vec![
        (vec!["edit", "{}", "-s", "a", "b"], "a\n"),
        (vec!["edit", "{}", "-e", "a", "b"], "a\n"),
        (
            vec!["edit", "{}", "--lines", "1:1", "--with-text", "b"],
            "a\n",
        ),
        (vec!["json", "set", ".a", "2", "{}"], "{\"a\": 1}\n"),
        (vec!["json", "del", ".a", "{}"], "{\"a\": 1}\n"),
        (vec!["json", "merge", "{}", "ro/o.json"], "{\"a\": 1}\n"),
        (vec!["yaml", "set", ".a", "2", "{}"], "a: 1\n"),
        (vec!["yaml", "del", ".a", "{}"], "a: 1\nb: 2\n"),
        (vec!["toml", "set", ".a", "2", "{}"], "a = 1\n"),
    ]
}

#[test]
fn every_writer_is_refused_outside_grants_and_through_symlinks() {
    for (argv, content) in writers() {
        let mut g = G::new();
        g.put("ro/o.json", "{\"b\": 2}");
        let merge_other = g.abs("ro/o.json");
        let fix = |argv: &[&'static str], file: &str| -> Vec<String> {
            argv.iter()
                .map(|a| match *a {
                    "{}" => file.to_string(),
                    "ro/o.json" => merge_other.clone(),
                    _ => a.to_string(),
                })
                .collect()
        };
        for (dir, why) in [
            ("ro", "outside the session's grants"),
            ("out", "outside the session's grants"),
        ] {
            let rel = format!("{dir}/f");
            g.put(&rel, content);
            let target = g.abs(&rel);
            let args = fix(&argv, &target);
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let (st, out, err) = g.run(&refs);
            assert_eq!(
                (st, out.as_str(), err),
                (1, "", format!("kapsel: {target}: {why}\n")),
                "{argv:?} in {dir}"
            );
            assert_eq!(g.get(&rel), content, "{argv:?} touched {rel}");
        }
        // A symlink inside the grant, to a file inside the grant: refused.
        g.put("work/real", content);
        symlink(g.abs("work/real"), g.abs("work/link")).unwrap();
        let args = fix(&argv, "link");
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (st, _, err) = g.run(&refs);
        assert_eq!(
            (st, err.as_str()),
            (1, "kapsel: link: refusing to write through symlink\n"),
            "{argv:?} through a symlink"
        );
        assert_eq!(
            g.get("work/real"),
            content,
            "{argv:?} wrote through the link"
        );
        // Control: the plain file in the grant is edited.
        g.put("work/plain", content);
        let args = fix(&argv, "plain");
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (st, _, err) = g.run(&refs);
        assert_eq!((st, err.as_str()), (0, ""), "{argv:?} control");
        assert_ne!(g.get("work/plain"), content, "{argv:?} control unchanged");
    }
}

#[test]
fn readers_are_refused_outside_grants() {
    let mut g = G::new();
    g.put("out/s", "{\"a\": 1}\n");
    let s = g.abs("out/s");
    for argv in [
        vec!["json", "get", ".a", s.as_str()],
        vec!["yaml", "get", ".a", s.as_str()],
        vec!["toml", "get", ".a", s.as_str()],
        vec!["field", "1", s.as_str()],
        vec!["lines", "1:2", s.as_str()],
        vec!["base64", s.as_str()],
        vec!["sha256sum", s.as_str()],
    ] {
        let (st, out, err) = g.run(&argv);
        assert_eq!(
            (st, out.as_str(), err),
            (
                1,
                "",
                format!("kapsel: {s}: outside the session's grants\n")
            ),
            "{argv:?}"
        );
    }
    // edit --with reads FILE2 under the read grant; json merge reads OTHER.
    g.put("work/f", "a\n");
    let (st, _, err) = g.run(&["edit", "f", "--lines", "1:1", "--with", s.as_str()]);
    assert_eq!(
        (st, err),
        (1, format!("kapsel: {s}: outside the session's grants\n"))
    );
    assert_eq!(g.get("work/f"), "a\n");
    g.put("work/j", "{}\n");
    let (st, _, err) = g.run(&["json", "merge", "j", s.as_str()]);
    assert_eq!(
        (st, err),
        (1, format!("kapsel: {s}: outside the session's grants\n"))
    );
    // sha256sum -c reads each listed file under the grants too.
    g.put("work/sums", &format!("{}  {s}\n", "0".repeat(64)));
    let (st, out, err) = g.run(&["sha256sum", "-c", "sums"]);
    assert_eq!(st, 1);
    assert_eq!(out, format!("{s}: FAILED open or read\n"));
    assert!(
        err.starts_with(&format!("kapsel: {s}: outside the session's grants\n")),
        "{err}"
    );
}

/// kapsel-check and the host parse the REQ-KS-063 flags with one table.
#[test]
fn xflags_tables_agree() {
    for name in [
        "edit",
        "json",
        "yaml",
        "toml",
        "field",
        "lines",
        "base64",
        "sha256sum",
    ] {
        let check: Vec<(&str, usize)> = kapsel_check::builtin_xflags(name)
            .unwrap()
            .iter()
            .map(|(f, v)| (*f, v.len()))
            .collect();
        let host = kapsel_host::xflags_for(name).unwrap().to_vec();
        assert_eq!(check, host, "{name}");
    }
}

#[test]
fn yaml_edge_layouts() {
    let mut t = T::new();
    // A nested block mapping in the middle is deleted whole, comments around it stay.
    t.write(
        "a.yaml",
        "a: 1\n# about b\nb:\n  x: 1\n  y:\n    - 2\nc: 3\n",
    );
    t.check(&["yaml", "del", ".b", "a.yaml"], "", 0, "", "");
    assert_eq!(t.read("a.yaml"), "a: 1\n# about b\nc: 3\n");
    // No trailing newline.
    t.write("b.yaml", "a: 1");
    t.check(&["yaml", "set", ".b", "2", "b.yaml"], "", 0, "", "");
    assert_eq!(t.read("b.yaml"), "a: 1\nb: 2");
    // An empty file, and one with only a comment.
    t.write("c.yaml", "");
    t.check(&["yaml", "set", ".a.b", "x", "c.yaml"], "", 0, "", "");
    assert_eq!(t.read("c.yaml"), "a:\n  b: x\n");
    t.write("d.yaml", "# empty\n");
    t.check(&["yaml", "set", ".a", "1", "d.yaml"], "", 0, "", "");
    assert_eq!(t.read("d.yaml"), "# empty\na: 1\n");
    // A null value gets one; a key with only a comment keeps the comment.
    t.write("e.yaml", "a:\nb: # later\nc: 1\n");
    t.check(&["yaml", "set", ".a", "1", "e.yaml"], "", 0, "", "");
    t.check(&["yaml", "set", ".b", "2", "e.yaml"], "", 0, "", "");
    assert_eq!(t.read("e.yaml"), "a: 1\nb: 2 # later\nc: 1\n");
    // A block sequence value replaced by a scalar.
    t.write("f.yaml", "l:\n  - 1\n  - 2\nz: 0\n");
    t.check(&["yaml", "set", ".l", "none", "f.yaml"], "", 0, "", "");
    assert_eq!(t.read("f.yaml"), "l: none\nz: 0\n");
    // Strings that would read back differently are quoted.
    t.write("g.yaml", "a: x\n");
    for v in ["1", "true", "on", "a: b", "x #y", "[1]", "", "-x", "it's"] {
        // `--` ends the flags, so a VALUE like `-x` is an operand.
        t.check(
            &["yaml", "set", "--str", "--", ".a", v, "g.yaml"],
            "",
            0,
            "",
            "",
        );
        let (st, out, _) = t.run(&["yaml", "get", "-j", ".a", "g.yaml"]);
        let want = format!("\"{}\"\n", v.replace('"', "\\\""));
        assert_eq!((st, out), (0, want), "{v:?}");
    }
    // A document start marker and quoted keys.
    t.write("h.yaml", "---\n\"a b\": 1\n'c': 2\n");
    t.check(&["yaml", "set", ".\"a b\"", "3", "h.yaml"], "", 0, "", "");
    t.check(&["yaml", "set", ".c", "4", "h.yaml"], "", 0, "", "");
    assert_eq!(t.read("h.yaml"), "---\n\"a b\": 3\n'c': 4\n");
}

/// Values outside the edited node stay byte-identical, unusual numbers
/// included; the edited value is written as typed.
#[test]
fn yaml_unusual_numbers_survive() {
    let mut t = T::new();
    let src =
        "v: 1.10\nhex: 0x1F\nsep: 1_000\noct: 0o17\nexp: 1e400\nneg: -0\nn: 1\nl:\n  - 1.10\n";
    t.write("n.yaml", src);
    t.check(&["yaml", "set", ".n", "2.50", "n.yaml"], "", 0, "", "");
    assert_eq!(t.read("n.yaml"), src.replace("n: 1\n", "n: 2.50\n"));
    t.check(&["yaml", "set", ".x", "0x10", "n.yaml"], "", 0, "", "");
    t.check(&["yaml", "set", ".l[1]", "1.20", "n.yaml"], "", 0, "", "");
    t.check(&["yaml", "del", ".n", "n.yaml"], "", 0, "", "");
    assert_eq!(
        t.read("n.yaml"),
        "v: 1.10\nhex: 0x1F\nsep: 1_000\noct: 0o17\nexp: 1e400\nneg: -0\nl:\n  - 1.10\n  - 1.20\nx: 0x10\n"
    );
    t.check(&["yaml", "get", ".v", "n.yaml"], "", 0, "1.1\n", "");
    // rivet-yaml v0.43.3 cannot read an integer past u64: refused, untouched.
    let huge = "huge: 123456789012345678901234567890\nn: 1\n";
    t.write("h.yaml", huge);
    let (st, _, err) = t.run(&["yaml", "set", ".n", "2", "h.yaml"]);
    assert_eq!(st, 1, "{err}");
    assert!(err.starts_with("yaml: h.yaml: invalid YAML: "), "{err}");
    assert_eq!(t.read("h.yaml"), huge);
}

#[test]
fn toml_unusual_numbers_survive() {
    let mut t = T::new();
    let src = "v = 1.10\nhex = 0x1F\nsep = 1_000\noct = 0o17\nexp = 1e300\nneg = -0.0\nn = 1\n\n[t]\nx = [1.10, 0x1F] # keep\n";
    t.write("n.toml", src);
    t.check(&["toml", "set", ".n", "2.50", "n.toml"], "", 0, "", "");
    assert_eq!(t.read("n.toml"), src.replace("n = 1\n", "n = 2.50\n"));
    t.check(&["toml", "set", ".t.y", "1_000", "n.toml"], "", 0, "", "");
    t.check(&["toml", "set", ".t.x[2]", "0b11", "n.toml"], "", 0, "", "");
    assert_eq!(
        t.read("n.toml"),
        "v = 1.10\nhex = 0x1F\nsep = 1_000\noct = 0o17\nexp = 1e300\nneg = -0.0\nn = 2.50\n\n[t]\nx = [1.10, 0x1F, 0b11] # keep\ny = 1_000\n"
    );
    t.check(&["toml", "get", ".hex", "n.toml"], "", 0, "0x1F\n", "");
}
