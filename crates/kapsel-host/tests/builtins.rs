//! Golden stdout/stderr/status per built-in and flag (REQ-KS-025).
//!
//! The expected values are what GNU coreutils/grep/sed produce in the C locale
//! (written by hand: this Mac has no GNU tools). CI on Ubuntu will diff the
//! same cases against the real tools later (docs/first-shell.md, Oracles).

mod common;
use common::T;

const NUMS: &str = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n";

#[test]
fn true_false_colon() {
    let mut t = T::new();
    t.check(&["true"], "", 0, "", "");
    t.check(&["false"], "", 1, "", "");
    t.check(&[":"], "", 0, "", "");
}

#[test]
fn echo_cases() {
    let mut t = T::new();
    t.check(&["echo", "hello", "world"], "", 0, "hello world\n", "");
    t.check(&["echo", "-n", "a"], "", 0, "a", "");
    t.check(&["echo", "-e", "a\\tb\\n"], "", 0, "a\tb\n\n", "");
    t.check(&["echo", "-x"], "", 0, "-x\n", "");
    t.check(&["echo", "-ne", "a\\cb"], "", 0, "a", "");
    t.check(&["echo", ""], "", 0, "\n", "");
    t.check(&["echo", "-E", "a\\nb"], "", 0, "a\\nb\n", "");
    t.check(&["echo", "-e", "\\0101"], "", 0, "A\n", "");
}

#[test]
fn printf_cases() {
    let mut t = T::new();
    t.check(&["printf", "%s\\n", "a", "b"], "", 0, "a\nb\n", "");
    t.check(&["printf", "%d %5d|%-5d|%05d\\n", "1", "2", "3", "4"], "", 0, "1     2|3    |00004\n", "");
    t.check(&["printf", "%x %X %o\\n", "255", "255", "8"], "", 0, "ff FF 10\n", "");
    t.check(&["printf", "%5.2f|%e\\n", "3.14159", "1234.5"], "", 0, " 3.14|1.234500e+03\n", "");
    t.check(&["printf", "%c%c\\n", "abc", "xyz"], "", 0, "ax\n", "");
    t.check(&["printf", "a\\tb\\n"], "", 0, "a\tb\n", "");
    t.check(&["printf", "%s\\n"], "", 0, "\n", "");
    t.check(&["printf", "%d\\n", "abc"], "", 1, "0\n", "printf: 'abc': expected a numeric value\n");
    t.check(&["printf", "%g %g %g\\n", "100000", "1000000", "0.0001"], "", 0, "100000 1e+06 0.0001\n", "");
    t.check(&["printf", "%b", "a\\nb"], "", 0, "a\nb", "");
    t.check(&["printf", "%%\\n"], "", 0, "%\n", "");
    t.check(&["printf", "%s=%d\\n", "a", "1", "b", "2"], "", 0, "a=1\nb=2\n", "");
    t.check(&["printf", "%.3s|\\n", "abcdef"], "", 0, "abc|\n", "");
    t.check(&["printf"], "", 2, "", "printf: missing operand\n");
}

#[test]
fn cat_cases() {
    let mut t = T::new();
    t.write("a.txt", "one\ntwo\n");
    t.write("b.txt", "three\n");
    t.check(&["cat", "a.txt", "b.txt"], "", 0, "one\ntwo\nthree\n", "");
    t.check(&["cat", "-n", "a.txt", "b.txt"], "", 0, "     1\tone\n     2\ttwo\n     3\tthree\n", "");
    t.check(&["cat"], "from stdin\n", 0, "from stdin\n", "");
    t.check(&["cat", "-", "b.txt"], "x\n", 0, "x\nthree\n", "");
    t.check(&["cat", "missing.txt"], "", 1, "", "cat: missing.txt: No such file or directory\n");
    t.check(&["cat", "missing.txt", "b.txt"], "", 1, "three\n", "cat: missing.txt: No such file or directory\n");
    t.write("blank.txt", "a\n\n\n\nb\n");
    t.check(&["cat", "-s", "blank.txt"], "", 0, "a\n\nb\n", "");
    t.check(&["cat", "-b", "blank.txt"], "", 0, "     1\ta\n\n\n\n     2\tb\n", "");
    t.check(&["cat", "-E", "b.txt"], "", 0, "three$\n", "");
    t.check(&["cat", "."], "", 1, "", "cat: .: Is a directory\n");
}

#[test]
fn wc_cases() {
    let mut t = T::new();
    t.write("a.txt", "one two\nthree\n");
    t.write("b.txt", "x\n");
    t.check(&["wc", "-l", "a.txt"], "", 0, "2 a.txt\n", "");
    t.check(&["wc", "-w", "a.txt"], "", 0, "3 a.txt\n", "");
    t.check(&["wc", "-c", "a.txt"], "", 0, "14 a.txt\n", "");
    t.check(&["wc", "a.txt"], "", 0, " 2  3 14 a.txt\n", "");
    t.check(&["wc", "a.txt", "b.txt"], "", 0, " 2  3 14 a.txt\n 1  1  2 b.txt\n 3  4 16 total\n", "");
    t.check(&["wc", "-c"], "hello\n", 0, "6\n", "");
    t.check(&["wc", "-l"], "a\nb\nc\n", 0, "3\n", "");
    t.check(&["wc"], "a b\n", 0, "      1       2       4\n", "");
    t.check(&["wc", "-l", "a.txt", "b.txt"], "", 0, " 2 a.txt\n 1 b.txt\n 3 total\n", "");
    t.check(&["wc", "-l", "missing"], "", 1, "", "wc: missing: No such file or directory\n");
}

#[test]
fn head_tail_cases() {
    let mut t = T::new();
    t.write("n.txt", NUMS);
    t.check(&["head", "n.txt"], "", 0, "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n", "");
    t.check(&["head", "-n", "3", "n.txt"], "", 0, "1\n2\n3\n", "");
    t.check(&["head", "-3", "n.txt"], "", 0, "1\n2\n3\n", "");
    t.check(&["head", "-n", "-10", "n.txt"], "", 0, "1\n2\n", "");
    t.check(&["head", "-c", "5"], "abcdefgh\n", 0, "abcde", "");
    t.check(&["head", "-n1"], "x\ny\n", 0, "x\n", "");
    t.check(&["tail", "-n", "3", "n.txt"], "", 0, "10\n11\n12\n", "");
    t.check(&["tail", "-n", "+11", "n.txt"], "", 0, "11\n12\n", "");
    t.check(&["tail", "-2", "n.txt"], "", 0, "11\n12\n", "");
    t.check(&["tail", "-c", "3"], "abcdef\n", 0, "ef\n", "");
    t.check(&["tail"], "1\n2\n", 0, "1\n2\n", "");
    t.write("one.txt", "1\n");
    t.write("two.txt", "2\n");
    t.check(&["head", "-n", "1", "one.txt", "two.txt"], "", 0, "==> one.txt <==\n1\n\n==> two.txt <==\n2\n", "");
    t.check(&["tail", "-q", "-n", "1", "one.txt", "two.txt"], "", 0, "1\n2\n", "");
    t.check(&["head", "-n", "x", "n.txt"], "", 1, "", "head: invalid number of lines: 'x'\n");
    t.check(&["head", "missing"], "", 1, "", "head: missing: No such file or directory\n");
}

#[test]
fn grep_cases() {
    let mut t = T::new();
    t.write("f.txt", "foo\nbar\nFoo baz\nfoo2\n");
    t.check(&["grep", "foo", "f.txt"], "", 0, "foo\nfoo2\n", "");
    t.check(&["grep", "-n", "foo", "f.txt"], "", 0, "1:foo\n4:foo2\n", "");
    t.check(&["grep", "-i", "foo", "f.txt"], "", 0, "foo\nFoo baz\nfoo2\n", "");
    t.check(&["grep", "-v", "foo", "f.txt"], "", 0, "bar\nFoo baz\n", "");
    t.check(&["grep", "-c", "foo", "f.txt"], "", 0, "2\n", "");
    t.check(&["grep", "-o", "fo*", "f.txt"], "", 0, "foo\nfoo\n", "");
    t.check(&["grep", "-l", "foo", "f.txt"], "", 0, "f.txt\n", "");
    t.check(&["grep", "-q", "foo", "f.txt"], "", 0, "", "");
    t.check(&["grep", "-q", "nomatch", "f.txt"], "", 1, "", "");
    t.check(&["grep", "-F", "o2", "f.txt"], "", 0, "foo2\n", "");
    t.check(&["grep", "-E", "foo|bar", "f.txt"], "", 0, "foo\nbar\nfoo2\n", "");
    t.check(&["grep", "-e", "bar", "-e", "baz", "f.txt"], "", 0, "bar\nFoo baz\n", "");
    t.check(&["grep", "nomatch", "f.txt"], "", 1, "", "");
    t.check(&["grep", "foo"], "foo\nx\n", 0, "foo\n", "");
    t.check(&["grep", "-n", "foo", "missing.txt"], "", 2, "", "grep: missing.txt: No such file or directory\n");
    // BRE: \+ is the repetition, a bare ( is literal.
    t.check(&["grep", "fo\\+", "f.txt"], "", 0, "foo\nfoo2\n", "");
    t.check(&["grep", "a(", "f.txt"], "", 1, "", "");
    t.check(&["grep", "-E", "a(", "f.txt"], "", 2, "", "grep: invalid regex 'a(': unclosed group\n");
    t.check(&["grep", "-c", "-v", "foo"], "foo\nbar\n", 0, "1\n", "");
    t.check(&["grep", "-x", "foo"], "foo\nfoo2\n", 0, "foo\n", "");
    t.check(&["grep", "-w", "foo"], "foo\nfoo2\na foo b\n", 0, "foo\na foo b\n", "");
    t.write("g.txt", "foo in g\n");
    t.check(&["grep", "foo", "f.txt", "g.txt"], "", 0, "f.txt:foo\nf.txt:foo2\ng.txt:foo in g\n", "");
    t.check(&["grep", "-c", "foo", "f.txt", "g.txt"], "", 0, "f.txt:2\ng.txt:1\n", "");
    t.check(&["grep", "-h", "foo", "f.txt", "g.txt"], "", 0, "foo\nfoo2\nfoo in g\n", "");
    t.check(&["grep", "-lv", "foo", "f.txt", "g.txt"], "", 0, "f.txt\n", "");
}

#[test]
fn grep_recursive() {
    let mut t = T::new();
    t.write("d/a.txt", "foo\n");
    t.write("d/sub/b.txt", "foo\nbar\n");
    t.write("d/sub/c.txt", "nothing\n");
    t.check(&["grep", "-r", "foo", "d"], "", 0, "d/a.txt:foo\nd/sub/b.txt:foo\n", "");
    t.check(&["grep", "-rn", "bar", "d"], "", 0, "d/sub/b.txt:2:bar\n", "");
    t.check(&["grep", "-rl", "foo", "d"], "", 0, "d/a.txt\nd/sub/b.txt\n", "");
    t.check(&["grep", "-r", "foo"], "", 0, "d/a.txt:foo\nd/sub/b.txt:foo\n", "");
    t.check(&["grep", "foo", "d"], "", 2, "", "grep: d: Is a directory\n");
}

#[test]
fn grep_unsupported_flag_is_an_error() {
    let mut t = T::new();
    let (st, out, err) = t.run_in(&["grep", "-A", "2", "foo"], "foo\n");
    assert_eq!(st, 2);
    assert_eq!(out, "");
    assert!(err.starts_with("grep: unknown flag -A; kapsel's grep supports: -E -F"), "{err}");
}

#[test]
fn sort_cases() {
    let mut t = T::new();
    t.check(&["sort"], "b\na\nc\na\n", 0, "a\na\nb\nc\n", "");
    t.check(&["sort", "-u"], "b\na\nc\na\n", 0, "a\nb\nc\n", "");
    t.check(&["sort", "-r"], "b\na\nc\na\n", 0, "c\nb\na\na\n", "");
    t.check(&["sort", "-n"], "10\n9\n100\n", 0, "9\n10\n100\n", "");
    t.check(&["sort", "-nr"], "10\n9\n100\n", 0, "100\n10\n9\n", "");
    t.check(&["sort"], "10\n9\n100\n", 0, "10\n100\n9\n", "");
    t.check(&["sort", "-k2"], "x 3\ny 1\nz 2\n", 0, "y 1\nz 2\nx 3\n", "");
    t.check(&["sort", "-k2n"], "x 10\ny 9\nz 100\n", 0, "y 9\nx 10\nz 100\n", "");
    t.check(&["sort", "-n", "-k", "2"], "x 10\ny 9\n", 0, "y 9\nx 10\n", "");
    t.check(&["sort", "-t,", "-k2"], "a,2\nb,1\n", 0, "b,1\na,2\n", "");
    t.check(&["sort", "-u", "-k1,1"], "a 1\na 2\nb 1\n", 0, "a 1\nb 1\n", "");
    t.check(&["sort", "-k1,1", "-r"], "a 1\nb 2\n", 0, "b 2\na 1\n", "");
    t.check(&["sort"], "no newline", 0, "no newline\n", "");
    t.check(&["sort", "missing"], "", 2, "", "sort: missing: No such file or directory\n");
    t.write("s.txt", "z\ny\n");
    t.check(&["sort", "s.txt"], "", 0, "y\nz\n", "");
    t.check(&["sort", "-o", "out.txt", "s.txt"], "", 0, "", "");
    assert_eq!(t.read("out.txt"), "y\nz\n");
}

#[test]
fn uniq_cases() {
    let mut t = T::new();
    let input = "a\na\nb\nc\nc\nc\n";
    t.check(&["uniq"], input, 0, "a\nb\nc\n", "");
    t.check(&["uniq", "-c"], input, 0, "      2 a\n      1 b\n      3 c\n", "");
    t.check(&["uniq", "-d"], input, 0, "a\nc\n", "");
    t.check(&["uniq", "-u"], input, 0, "b\n", "");
    t.check(&["uniq", "-i"], "A\na\nb\n", 0, "A\nb\n", "");
    t.check(&["uniq"], "a\nb\na\n", 0, "a\nb\na\n", "");
}

#[test]
fn tee_cases() {
    let mut t = T::new();
    t.check(&["tee", "o.txt"], "hi\n", 0, "hi\n", "");
    assert_eq!(t.read("o.txt"), "hi\n");
    t.check(&["tee", "-a", "o.txt"], "more\n", 0, "more\n", "");
    assert_eq!(t.read("o.txt"), "hi\nmore\n");
    t.check(&["tee", "p.txt", "q.txt"], "x\n", 0, "x\n", "");
    assert_eq!(t.read("q.txt"), "x\n");
    t.check(&["tee"], "only out\n", 0, "only out\n", "");
}

#[test]
fn cut_cases() {
    let mut t = T::new();
    t.check(&["cut", "-d,", "-f2"], "a,b,c\n", 0, "b\n", "");
    t.check(&["cut", "-d", ",", "-f1,3"], "a,b,c\n", 0, "a,c\n", "");
    t.check(&["cut", "-d,", "-f2-"], "a,b,c\n", 0, "b,c\n", "");
    t.check(&["cut", "-d,", "-f-2"], "a,b,c\n", 0, "a,b\n", "");
    t.check(&["cut", "-c1-3"], "abcdef\n", 0, "abc\n", "");
    t.check(&["cut", "-d,", "-f2"], "nodelim\n", 0, "nodelim\n", "");
    t.check(&["cut", "-d,", "-f2", "-s"], "nodelim\na,b\n", 0, "b\n", "");
    t.check(&["cut", "-f2"], "a\tb\tc\n", 0, "b\n", "");
    t.check(&["cut", "-b2"], "abc\n", 0, "b\n", "");
    t.check(&["cut"], "", 2, "", "cut: you must specify a list of bytes, characters, or fields\n");
}

#[test]
fn tr_cases() {
    let mut t = T::new();
    t.check(&["tr", "a-z", "A-Z"], "hello\n", 0, "HELLO\n", "");
    t.check(&["tr", "-d", "l"], "hello\n", 0, "heo\n", "");
    t.check(&["tr", "-s", " "], "a   b  c\n", 0, "a b c\n", "");
    t.check(&["tr", "-d", "\\n"], "a\nb\n", 0, "ab", "");
    t.check(&["tr", "[:lower:]", "[:upper:]"], "abc\n", 0, "ABC\n", "");
    t.check(&["tr", "-cd", "0-9"], "a1b2\n", 0, "12", "");
    t.check(&["tr", "abc", "xy"], "abc\n", 0, "xyy\n", "");
    t.check(&["tr", "-d", "[:space:]"], "a b\tc\n", 0, "abc", "");
    t.check(&["tr", "a"], "", 2, "", "tr: missing operand after 'a'\n");
}

#[test]
fn seq_cases() {
    let mut t = T::new();
    t.check(&["seq", "3"], "", 0, "1\n2\n3\n", "");
    t.check(&["seq", "2", "4"], "", 0, "2\n3\n4\n", "");
    t.check(&["seq", "1", "2", "7"], "", 0, "1\n3\n5\n7\n", "");
    t.check(&["seq", "5", "-2", "1"], "", 0, "5\n3\n1\n", "");
    t.check(&["seq", "-s,", "3"], "", 0, "1,2,3\n", "");
    t.check(&["seq", "-w", "8", "10"], "", 0, "08\n09\n10\n", "");
    t.check(&["seq", "0.5", "0.5", "2"], "", 0, "0.5\n1.0\n1.5\n2.0\n", "");
    t.check(&["seq", "0"], "", 0, "", "");
    t.check(&["seq", "-3", "-1"], "", 0, "-3\n-2\n-1\n", "");
    t.check(&["seq", "x"], "", 2, "", "seq: invalid floating point argument: 'x'\n");
}

#[test]
fn sed_cases() {
    let mut t = T::new();
    let s = "one\ntwo\nthree\nfour\n";
    t.check(&["sed", "-n", "2p"], s, 0, "two\n", "");
    t.check(&["sed", "-n", "2,3p"], s, 0, "two\nthree\n", "");
    t.check(&["sed", "2d"], s, 0, "one\nthree\nfour\n", "");
    t.check(&["sed", "s/o/0/"], s, 0, "0ne\ntw0\nthree\nf0ur\n", "");
    t.check(&["sed", "s/o/0/g"], "foo\n", 0, "f00\n", "");
    t.check(&["sed", "-n", "/t/p"], s, 0, "two\nthree\n", "");
    t.check(&["sed", "/t/d"], s, 0, "one\nfour\n", "");
    t.check(&["sed", "-E", "s/(o)(n)/\\2\\1/"], s, 0, "noe\ntwo\nthree\nfour\n", "");
    t.check(&["sed", "-n", "$p"], s, 0, "four\n", "");
    t.check(&["sed", "2q"], s, 0, "one\ntwo\n", "");
    t.check(&["sed", "-n", "2,+1p"], s, 0, "two\nthree\n", "");
    t.check(&["sed", "s/e$/E/"], s, 0, "onE\ntwo\nthrEE\nfour\n".replace("thrEE", "threE").as_str(), "");
    t.check(&["sed", "s/a\\|b/X/g"], "abc\n", 0, "XXc\n", "");
    t.check(&["sed", "s/(a)/b/"], "(a)\n", 0, "b\n", "");
    t.check(&["sed", "-e", "s/one/1/", "-e", "s/two/2/"], s, 0, "1\n2\nthree\nfour\n", "");
    t.check(&["sed", "s/o/&&/"], "foo\n", 0, "fooo\n", "");
    t.check(&["sed", "s|/|-|g"], "a/b/c\n", 0, "a-b-c\n", "");
    t.check(&["sed", "-n", "s/o/0/p"], s, 0, "0ne\ntw0\nf0ur\n", "");
    t.check(&["sed", "s/b/c/"], "a\nb", 0, "a\nc", "");
    t.check(&["sed", "2!d"], s, 0, "two\n", "");
    t.check(&["sed", "/one/,/three/d"], s, 0, "four\n", "");
    t.write("in.txt", "hello\nworld\n");
    t.check(&["sed", "-i", "s/o/0/g", "in.txt"], "", 0, "", "");
    assert_eq!(t.read("in.txt"), "hell0\nw0rld\n");
    t.check(&["sed", "-i.bak", "s/l/L/", "in.txt"], "", 0, "", "");
    assert_eq!(t.read("in.txt"), "heLl0\nw0rLd\n");
    assert_eq!(t.read("in.txt.bak"), "hell0\nw0rld\n");
    t.check(&["sed", "s/x/y/", "missing"], "", 2, "", "sed: missing: No such file or directory\n");
}

#[test]
fn sed_unsupported_command_names_it() {
    let mut t = T::new();
    let (st, _, err) = t.run_in(&["sed", "y/a/b/"], "a\n");
    assert_eq!(st, 2);
    assert!(err.contains("unsupported command 'y'"), "{err}");
    let (st, _, err) = t.run_in(&["sed", "-E", "s/a(/b/"], "a\n");
    assert_eq!(st, 2);
    assert!(err.contains("invalid regex"), "{err}");
}

#[test]
fn mkdir_cases() {
    let mut t = T::new();
    t.check(&["mkdir", "d"], "", 0, "", "");
    assert!(t.path("d").is_dir());
    t.check(&["mkdir", "d"], "", 1, "", "mkdir: cannot create directory 'd': File exists\n");
    t.check(&["mkdir", "-p", "a/b/c"], "", 0, "", "");
    assert!(t.path("a/b/c").is_dir());
    t.check(&["mkdir", "-p", "a/b/c"], "", 0, "", "");
    t.check(&["mkdir", "x/y"], "", 1, "", "mkdir: cannot create directory 'x/y': No such file or directory\n");
    t.check(&["mkdir"], "", 1, "", "mkdir: missing operand\n");
}

#[test]
fn rm_cases() {
    let mut t = T::new();
    t.write("f", "x");
    t.check(&["rm", "f"], "", 0, "", "");
    assert!(!t.exists("f"));
    t.check(&["rm", "missing"], "", 1, "", "rm: cannot remove 'missing': No such file or directory\n");
    t.check(&["rm", "-f", "missing"], "", 0, "", "");
    t.write("d/x", "x");
    t.check(&["rm", "d"], "", 1, "", "rm: cannot remove 'd': Is a directory\n");
    t.check(&["rm", "-r", "d"], "", 0, "", "");
    assert!(!t.exists("d"));
    t.write("e/f/g", "x");
    t.check(&["rm", "-rf", "e"], "", 0, "", "");
    assert!(!t.exists("e"));
    t.check(&["rm", "-r", "."], "", 1, "", "rm: refusing to remove '.' or '..' directory: skipping '.'\n");
    t.check(&["rm"], "", 1, "", "rm: missing operand\n");
}

#[test]
fn cp_cases() {
    let mut t = T::new();
    t.write("a", "A");
    t.check(&["cp", "a", "b"], "", 0, "", "");
    assert_eq!(t.read("b"), "A");
    t.check(&["cp", "missing", "c"], "", 1, "", "cp: cannot stat 'missing': No such file or directory\n");
    t.write("d/x", "X");
    t.write("d/s/y", "Y");
    t.check(&["cp", "d", "d2"], "", 1, "", "cp: -r not specified; omitting directory 'd'\n");
    t.check(&["cp", "-r", "d", "d2"], "", 0, "", "");
    assert_eq!(t.read("d2/s/y"), "Y");
    t.check(&["cp", "-r", "d", "d2"], "", 0, "", "");
    assert_eq!(t.read("d2/d/x"), "X");
    t.check(&["cp", "a", "b", "d"], "", 0, "", "");
    assert_eq!(t.read("d/b"), "A");
    t.check(&["cp", "a", "b", "nodir"], "", 1, "", "cp: target 'nodir' is not a directory\n");
    t.check(&["cp", "a", "a"], "", 1, "", "cp: 'a' and 'a' are the same file\n");
    t.check(&["cp", "a"], "", 1, "", "cp: missing destination file operand after 'a'\n");
}

#[test]
fn mv_cases() {
    let mut t = T::new();
    t.write("a", "A");
    t.check(&["mv", "a", "b"], "", 0, "", "");
    assert!(!t.exists("a"));
    assert_eq!(t.read("b"), "A");
    t.write("d/keep", "k");
    t.check(&["mv", "b", "d"], "", 0, "", "");
    assert_eq!(t.read("d/b"), "A");
    t.check(&["mv", "missing", "z"], "", 1, "", "mv: cannot stat 'missing': No such file or directory\n");
    t.write("e/x", "x");
    t.check(&["mv", "e", "f"], "", 0, "", "");
    assert_eq!(t.read("f/x"), "x");
}

#[test]
fn ls_cases() {
    let mut t = T::new();
    t.write("b", "");
    t.write("a", "");
    t.write(".h", "");
    t.check(&["ls"], "", 0, "a\nb\n", "");
    t.check(&["ls", "-1"], "", 0, "a\nb\n", "");
    t.check(&["ls", "-a"], "", 0, ".\n..\n.h\na\nb\n", "");
    t.check(&["ls", "-A"], "", 0, ".h\na\nb\n", "");
    t.check(&["ls", "missing"], "", 2, "", "ls: cannot access 'missing': No such file or directory\n");
    t.check(&["ls", "a"], "", 0, "a\n", "");
    t.write("d/x", "");
    t.check(&["ls", "d", "a"], "", 0, "a\n\nd:\nx\n", "");
    t.check(&["ls", "-d", "d"], "", 0, "d\n", "");
    let (st, out, _) = t.run(&["ls", "-l", "a"]);
    assert_eq!(st, 0);
    let re = regex::Regex::new(r"^[-d][-rwx]{9} +1 \S+ +\S+ +0 [A-Z][a-z]{2} [ 0-9]\d \d{2}:\d{2} a\n$").unwrap();
    assert!(re.is_match(&out), "{out:?}");
    let (_, out, _) = t.run(&["ls", "-l", "d"]);
    assert!(out.starts_with("total "), "{out:?}");
}

#[test]
fn cd_pwd_export_exit() {
    let mut t = T::new();
    let root = t.root();
    t.check(&["pwd"], "", 0, &format!("{}\n", root.display()), "");
    t.write("d/x", "");
    t.check(&["cd", "d"], "", 0, "", "");
    assert_eq!(t.s.cwd(), root.join("d"));
    t.check(&["pwd"], "", 0, &format!("{}/d\n", root.display()), "");
    t.check(&["cat", "x"], "", 0, "", "");
    let root_s = root.display().to_string();
    t.check(&["cd", &root_s], "", 0, "", "");
    t.check(&["cd", "d/.."], "", 0, "", "");
    assert_eq!(t.s.cwd(), root);
    assert_eq!(t.s.cwd(), root);
    t.check(&["cd", "missing"], "", 1, "", "cd: missing: No such file or directory\n");
    t.write("f", "");
    t.check(&["cd", "f"], "", 1, "", "cd: f: Not a directory\n");
    t.check(&["cd", "d"], "", 0, "", "");
    t.check(&["cd", "-"], "", 0, &format!("{}\n", root.display()), "");
    t.check(&["cd"], "", 0, "", ""); // HOME is the root
    t.check(&["export", "A=1", "B"], "", 0, "", "");
    assert_eq!(t.s.var("A").as_deref(), Some("1"));
    assert!(t.s.is_exported("A") && t.s.is_exported("B"));
    t.check(&["export", "1x"], "", 1, "", "export: '1x': not a valid identifier\n");
    assert_eq!(t.s.call_direct(&["exit", "3"]), 259);
    assert_eq!(t.s.call_direct(&["exit", "0"]), 256);
    assert_eq!(t.s.call_direct(&["exit", "300"]), 256 + 44);
    t.s.call_direct(&["false"]);
    assert_eq!(t.s.call_direct(&["exit"]), 257);
    t.check(&["exit", "abc"], "", 258, "", "exit: abc: numeric argument required\n");
}

#[test]
fn test_builtin() {
    let mut t = T::new();
    t.write("a", "x");
    t.write("empty", "");
    t.write("d/y", "");
    let ok = |t: &mut T, args: &[&str], want: u32| {
        let (st, _, err) = t.run(args);
        assert_eq!(st, want, "{args:?} {err}");
    };
    ok(&mut t, &["[", "-f", "a", "]"], 0);
    ok(&mut t, &["[", "-d", "a", "]"], 1);
    ok(&mut t, &["[", "-d", "d", "]"], 0);
    ok(&mut t, &["[", "-e", "missing", "]"], 1);
    ok(&mut t, &["[", "-s", "a", "]"], 0);
    ok(&mut t, &["[", "-s", "empty", "]"], 1);
    ok(&mut t, &["[", "1", "-lt", "2", "]"], 0);
    ok(&mut t, &["[", "2", "-le", "1", "]"], 1);
    ok(&mut t, &["[", "a", "=", "a", "]"], 0);
    ok(&mut t, &["[", "a", "!=", "a", "]"], 1);
    ok(&mut t, &["[", "-z", "", "]"], 0);
    ok(&mut t, &["[", "-n", "", "]"], 1);
    ok(&mut t, &["[", "!", "-f", "nope", "]"], 0);
    ok(&mut t, &["[", "-f", "a", "-a", "-d", "d", "]"], 0);
    ok(&mut t, &["[", "-f", "nope", "-o", "-d", "d", "]"], 0);
    ok(&mut t, &["[", "(", "-f", "a", ")", "]"], 0);
    ok(&mut t, &["test", "-f", "a"], 0);
    ok(&mut t, &["test"], 1);
    ok(&mut t, &["test", "x"], 0);
    ok(&mut t, &["test", ""], 1);
    t.check(&["[", "-f", "a"], "", 2, "", "[: missing ']'\n");
    t.check(&["[", "x", "-eq", "1", "]"], "", 2, "", "[: x: integer expression expected\n");
    let (st, _, err) = t.run(&["test", "a", "-nt", "b"]);
    assert_eq!(st, 2);
    assert!(err.contains("unsupported operator '-nt'"), "{err}");
}

#[test]
fn date_and_sleep() {
    let mut t = T::new();
    t.check(&["date", "-u", "-d", "@0", "+%Y-%m-%d %H:%M:%S"], "", 0, "1970-01-01 00:00:00\n", "");
    t.check(&["date", "-d", "@86400", "+%a"], "", 0, "Fri\n", "");
    t.check(&["date", "-d", "@1700000000", "+%F %T %A %j %b %e"], "", 0, "2023-11-14 22:13:20 Tuesday 318 Nov 14\n", "");
    t.check(&["date", "-d", "@0"], "", 0, "Thu Jan  1 00:00:00 UTC 1970\n", "");
    t.check(&["date", "-d", "2024-02-29 12:30:05", "+%s"], "", 0, "1709209805\n", "");
    t.check(&["date", "-d", "nonsense"], "", 1, "", "date: invalid date 'nonsense'\n");
    let (st, out, _) = t.run(&["date", "+%Y"]);
    assert_eq!(st, 0);
    assert!(out.trim().parse::<u32>().unwrap() >= 2024);
    t.check(&["sleep", "0"], "", 0, "", "");
    t.check(&["sleep", "0.01"], "", 0, "", "");
    t.check(&["sleep", "x"], "", 1, "", "sleep: invalid time interval 'x'\n");
    t.check(&["sleep"], "", 1, "", "sleep: missing operand\n");
}

#[test]
fn unknown_flags_are_errors_listing_the_supported_set() {
    let mut t = T::new();
    t.write("f", "x\n");
    let cases: &[(&str, &str)] = &[
        ("cat", "-Z"),
        ("wc", "-Z"),
        ("head", "-Z"),
        ("tail", "-f"),
        ("grep", "-Z"),
        ("sort", "-Z"),
        ("uniq", "-Z"),
        ("tee", "-Z"),
        ("cut", "-Z"),
        ("tr", "-Z"),
        ("seq", "-Z"),
        ("sed", "-Z"),
        ("mkdir", "-Z"),
        ("rm", "-Z"),
        ("cp", "-Z"),
        ("mv", "-Z"),
        ("ls", "-Z"),
        ("date", "-Z"),
        ("sleep", "-Z"),
        ("cd", "-Z"),
        ("pwd", "-Z"),
        ("export", "-Z"),
        ("cat", "--bogus"),
    ];
    for (cmd, flag) in cases {
        let (st, out, err) = t.run(&[cmd, flag, "f"]);
        assert_eq!(st, 2, "{cmd} {flag}: {err}");
        assert_eq!(out, "", "{cmd} {flag}");
        assert!(err.starts_with(&format!("{cmd}: unknown flag {flag}; ")), "{cmd} {flag}: {err}");
        assert!(err.contains("supports:") || err.contains("takes"), "{cmd} {flag}: {err}");
    }
    // The supported set is named in full.
    let (_, _, err) = t.run(&["grep", "-Z", "x", "f"]);
    assert_eq!(err, "grep: unknown flag -Z; kapsel's grep supports: -E -F -G -n -i -v -r -R -l -c -o -q -h -H -s -w -x -e PATTERN\n");
}

#[test]
fn every_builtin_name_dispatches() {
    // Each name in kapsel_plan::BUILTINS must be known to the host: an
    // unknown name would print "not a kapsel built-in".
    let mut t = T::new();
    for name in kapsel_host::BUILTINS {
        let (_, _, err) = t.run(&[name]);
        assert!(!err.contains("not a kapsel built-in"), "{name}: {err}");
    }
    let (st, _, err) = t.run(&["curl"]);
    assert_eq!((st, err.as_str()), (2, "curl: curl: not a kapsel built-in\n"));
}
