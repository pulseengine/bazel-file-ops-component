//! Canonical ABI layout of the arguments the compiled plan passes to the host,
//! and the data-segment allocator arithmetic.
//!
//! The lowering writes every field through the constants below, and the
//! layout proofs (`proofs/layout-*.smt2`) are generated from [`RECORDS`],
//! [`align_up`], [`pages_for`] and [`MAX_END`]. A test checks [`RECORDS`]
//! against wit-parser's canonical-ABI sizes and alignments for
//! `kapsel:shell/host@0.1.0`.

/// Address where the compile-time data segment starts.
pub const DATA_BASE: u32 = 16;
/// Address of the `expand-list` return area (`tuple<u32, u32>`).
pub const RET_AREA: u32 = 8;
/// Size of the return area.
pub const RET_AREA_SIZE: u32 = 8;
/// The data segment's last byte must sit at or below this address: the
/// allocator declines (KC-003) when `DATA_BASE + end` would exceed it.
pub const MAX_END: u64 = u32::MAX as u64;
/// Most elements in one list (argv words, parts of a word, assignments,
/// redirects, loop items). A longer list is declined (KC-003). The layout
/// proofs' element arithmetic is stated for lists up to this length.
pub const MAX_LIST: u32 = 65_536;
/// Linear memory page size.
pub const PAGE: u64 = 65_536;
/// log2 of [`PAGE`].
pub const PAGE_SHIFT: u32 = 16;

// `part` (variant, 4 cases: u8 discriminant, payload at 4): 16 bytes, align 4.
pub const PART_SIZE: u32 = 16;
pub const PART_DISC: u32 = 0;
/// lit / var: `text` / `name` string; glob: the pattern string.
pub const PART_STR_PTR: u32 = 4;
pub const PART_STR_LEN: u32 = 8;
/// lit / var: `quoted`.
pub const PART_QUOTED: u32 = 12;
/// captured: `pipe` and `quoted`.
pub const PART_CAP_PIPE: u32 = 4;
pub const PART_CAP_QUOTED: u32 = 8;

// `string` and `list<T>`: (ptr, len), 8 bytes, align 4. `word` = list<part>.
pub const WORD_SIZE: u32 = 8;
pub const LIST_PTR: u32 = 0;
pub const LIST_LEN: u32 = 4;

// `tuple<string, word>`: 16 bytes, align 4.
pub const ASSIGN_SIZE: u32 = 16;
pub const ASSIGN_NAME_PTR: u32 = 0;
pub const ASSIGN_NAME_LEN: u32 = 4;
pub const ASSIGN_WORD_PTR: u32 = 8;
pub const ASSIGN_WORD_LEN: u32 = 12;

// `redirect` record: 20 bytes, align 4.
pub const REDIRECT_SIZE: u32 = 20;
pub const REDIR_FD: u32 = 0;
pub const REDIR_OP: u32 = 4;
pub const REDIR_TARGET_PTR: u32 = 8;
pub const REDIR_TARGET_LEN: u32 = 12;
pub const REDIR_TARGET_FD: u32 = 16;

/// One field the lowering writes: offset, size and alignment inside its record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub offset: u32,
    pub size: u32,
    pub align: u32,
}

/// A list element type: its stride (size), alignment and fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub name: &'static str,
    pub size: u32,
    pub align: u32,
    pub fields: &'static [Field],
}

const fn f(name: &'static str, offset: u32, size: u32, align: u32) -> Field {
    Field {
        name,
        offset,
        size,
        align,
    }
}

/// Every list element layout the lowering writes, with every field.
pub const RECORDS: &[Record] = &[
    Record {
        name: "part",
        size: PART_SIZE,
        align: 4,
        fields: &[
            f("discriminant", PART_DISC, 1, 1),
            f("str.ptr", PART_STR_PTR, 4, 4),
            f("str.len", PART_STR_LEN, 4, 4),
            f("quoted", PART_QUOTED, 1, 1),
            f("captured.pipe", PART_CAP_PIPE, 4, 4),
            f("captured.quoted", PART_CAP_QUOTED, 1, 1),
        ],
    },
    Record {
        name: "word",
        size: WORD_SIZE,
        align: 4,
        fields: &[f("ptr", LIST_PTR, 4, 4), f("len", LIST_LEN, 4, 4)],
    },
    Record {
        name: "assignment",
        size: ASSIGN_SIZE,
        align: 4,
        fields: &[
            f("name.ptr", ASSIGN_NAME_PTR, 4, 4),
            f("name.len", ASSIGN_NAME_LEN, 4, 4),
            f("word.ptr", ASSIGN_WORD_PTR, 4, 4),
            f("word.len", ASSIGN_WORD_LEN, 4, 4),
        ],
    },
    Record {
        name: "redirect",
        size: REDIRECT_SIZE,
        align: 4,
        fields: &[
            f("fd", REDIR_FD, 4, 4),
            f("op", REDIR_OP, 1, 1),
            f("target.ptr", REDIR_TARGET_PTR, 4, 4),
            f("target.len", REDIR_TARGET_LEN, 4, 4),
            f("target-fd", REDIR_TARGET_FD, 4, 4),
        ],
    },
];

/// The allocator's rounding: `len` rounded up to `align`, a power of two.
pub const fn align_up(len: u64, align: u64) -> u64 {
    (len + align - 1) & !(align - 1)
}

/// Pages of linear memory for `total` bytes (at least one).
pub const fn pages_for(total: u64) -> u64 {
    let p = (total + PAGE - 1) >> PAGE_SHIFT;
    if p == 0 { 1 } else { p }
}
