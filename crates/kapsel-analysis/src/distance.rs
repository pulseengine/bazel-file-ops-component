//! Edit distance between paths, and the near-miss test the KS-S rules use.

/// Paths longer than this (in bytes) are never compared.
pub const MAX_PATH_LEN: usize = 4096;

/// Optimal-string-alignment distance over Unicode scalar values (insertions,
/// deletions, substitutions and adjacent transpositions each cost 1), capped:
/// returns `None` as soon as the distance must exceed `max`.
///
/// Memory is three rows of `len(b) + 1` cells; time is
/// O(len(a) * (2 * max + 1)), because only the diagonal band `|i - j| <= max`
/// can hold a value `<= max`. Inputs differing in length by more than `max`
/// return `None` without any table.
pub fn bounded_distance(a: &str, b: &str, max: usize) -> Option<usize> {
    if a.chars().count().abs_diff(b.chars().count()) > max {
        return None;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let inf = max + 1;
    // rows: i-2, i-1, i
    let mut pp = vec![inf; m + 1];
    let mut prev: Vec<usize> = (0..=m).map(|j| if j <= max { j } else { inf }).collect();
    let mut cur = vec![inf; m + 1];
    for i in 1..=n {
        let lo = i.saturating_sub(max).max(1);
        let hi = (i + max).min(m);
        // Only the band and one cell either side are written per row; the
        // next rows read nothing outside that, so no O(m) reset is needed.
        cur[0] = if i <= max { i } else { inf };
        cur[lo - 1] = if lo == 1 { cur[0] } else { inf };
        if hi < m {
            cur[hi + 1] = inf;
        }
        let mut row_min = if lo == 1 { cur[0] } else { inf };
        for j in lo..=hi {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(pp[j - 2] + 1);
            }
            cur[j] = v.min(inf);
            row_min = row_min.min(cur[j]);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut pp, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    Some(prev[m]).filter(|&d| d <= max)
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
    if a == b || a.len() > MAX_PATH_LEN || b.len() > MAX_PATH_LEN {
        return None;
    }
    let d = bounded_distance(a, b, max_distance(a, b))?;
    if d == 0 {
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

    /// Reference: the full-matrix OSA distance, for comparing with the band.
    fn full(a: &str, b: &str) -> usize {
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

    #[test]
    fn distances() {
        assert_eq!(bounded_distance("", "", 2), Some(0));
        assert_eq!(bounded_distance("abc", "abc", 2), Some(0));
        assert_eq!(bounded_distance("out/a.txt", "out/a.text", 2), Some(1));
        assert_eq!(bounded_distance("report", "reprot", 2), Some(1));
        assert_eq!(bounded_distance("kitten", "sitting", 2), None);
        assert_eq!(bounded_distance("kitten", "sitting", 3), Some(3));
        assert_eq!(bounded_distance("café", "cafe", 1), Some(1));
    }

    #[test]
    fn band_agrees_with_full_matrix() {
        let words = ["", "a", "ab", "ba", "abc", "acb", "out/a.txt", "out/a.text", "ou/a.txt", "data.csv", "dta.csv", "café", "cafe", "xyz"];
        for a in words {
            for b in words {
                let f = full(a, b);
                for max in 0..4 {
                    assert_eq!(bounded_distance(a, b, max), (f <= max).then_some(f), "{a:?} {b:?} max {max}");
                }
            }
        }
    }

    #[test]
    fn megabyte_inputs_are_bounded() {
        let a = "a".repeat(1 << 20);
        let mut b = a.clone();
        b.replace_range(500_000..500_001, "b");
        let t = std::time::Instant::now();
        // Over MAX_PATH_LEN: rejected before any work.
        assert_eq!(near_miss(&a, &b), None);
        // The band itself: 3 rows of 1M cells and ~5M cell updates, not a
        // 1M x 1M matrix.
        assert_eq!(bounded_distance(&a, &b, 2), Some(1));
        assert_eq!(bounded_distance(&a, &format!("{a}xyz"), 2), None);
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "took {:?}", t.elapsed());
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
            assert_eq!(bounded_distance(a, b, 2), bounded_distance(b, a, 2));
        }
    }
}
