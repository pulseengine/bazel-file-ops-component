//! Edit distance between paths, and the near-miss test the KS-S rules use.

/// Optimal-string-alignment distance over Unicode scalar values: insertions,
/// deletions, substitutions and adjacent transpositions each cost 1.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[n][m]
}

fn is_sep(c: Option<char>) -> bool {
    matches!(c, None | Some('/' | '.' | '_' | '-'))
}

/// The differing middle of `a` and `b` after removing their common prefix and
/// suffix, plus the characters just outside it.
fn diff_window(a: &str, b: &str) -> (String, String, Option<char>, Option<char>) {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut p = 0;
    while p < a.len() && p < b.len() && a[p] == b[p] {
        p += 1;
    }
    let mut s = 0;
    while s < a.len() - p && s < b.len() - p && a[a.len() - 1 - s] == b[b.len() - 1 - s] {
        s += 1;
    }
    let before = if p > 0 { Some(a[p - 1]) } else { None };
    let after = if s > 0 { Some(a[a.len() - s]) } else { None };
    (a[p..a.len() - s].iter().collect(), b[p..b.len() - s].iter().collect(), before, after)
}

/// Largest distance that still counts as a near miss for paths this long:
/// 1 below 6 characters, 2 from 6 on.
pub fn max_distance(a: &str, b: &str) -> usize {
    if a.chars().count().min(b.chars().count()) < 6 { 1 } else { 2 }
}

/// Returns the distance when `a` and `b` look like a typo of each other:
/// distance 1 or 2 (see [`max_distance`]), and not a deliberate variant.
///
/// Deliberate variants are excluded: a numbered series (`log1` vs `log2`,
/// the differing characters are all digits) and a whole-name swap (`out/a.txt`
/// vs `out/b.txt`, the difference is a complete name between separators).
pub fn near_miss(a: &str, b: &str) -> Option<usize> {
    if a == b {
        return None;
    }
    let d = edit_distance(a, b);
    if d == 0 || d > max_distance(a, b) {
        return None;
    }
    let (da, db, before, after) = diff_window(a, b);
    let all_digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if all_digits(&da) && all_digits(&db) {
        return None;
    }
    if !da.is_empty() && !db.is_empty() && is_sep(before) && is_sep(after) {
        return None;
    }
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("out/a.txt", "out/a.text"), 1);
        assert_eq!(edit_distance("report", "reprot"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("café", "cafe"), 1);
    }

    #[test]
    fn near_misses() {
        assert_eq!(near_miss("out/a.txt", "out/a.text"), Some(1));
        assert_eq!(near_miss("data.csv", "dta.csv"), Some(1));
        assert_eq!(near_miss("results/report.txt", "results/reprot.txt"), Some(1));
        assert_eq!(near_miss("build/output", "biuld/outptu"), Some(2));
    }

    #[test]
    fn deliberate_variants_are_not_near_misses() {
        assert_eq!(near_miss("log1.txt", "log2.txt"), None);
        assert_eq!(near_miss("out/a.txt", "out/b.txt"), None);
        assert_eq!(near_miss("a", "b"), None);
        assert_eq!(near_miss("x.txt", "x.txt"), None);
        assert_eq!(near_miss("abcd", "wxyz"), None);
        // Short names allow distance 1 only.
        assert_eq!(near_miss("ab.c", "ba.cd"), None);
    }

    #[test]
    fn symmetric_and_deterministic() {
        for (a, b) in [("out/a.txt", "out/a.text"), ("data.csv", "dta.csv"), ("log1", "log2")] {
            assert_eq!(near_miss(a, b), near_miss(b, a));
            assert_eq!(edit_distance(a, b), edit_distance(b, a));
        }
    }
}
