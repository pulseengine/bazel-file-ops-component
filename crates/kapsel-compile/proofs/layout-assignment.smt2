; kapsel-compile layout obligation: `assignment` fields (stride 16, align 4)
; fields: name.ptr@0:4/4, name.len@4:4/4, word.ptr@8:4/4, word.len@12:4/4
; for every element start aligned to 4 with start + 16 <= list end <= 2^32 (layout-stride-16)
; arithmetic at 34 bits: every input is below 2^32 and every intermediate below 2^33 (elem + offset + size), so nothing wraps
(set-logic QF_BV)
(declare-const elem (_ BitVec 34))
(declare-const list_end (_ BitVec 34))
(assert (and
  (= (bvand elem (_ bv3 34)) (_ bv0 34))
  (bvule elem (_ bv4294967296 34))
  (bvule list_end (_ bv4294967296 34))
  (bvule (bvadd elem (_ bv16 34)) list_end)
  (not (and
  (= (bvand (bvadd elem (_ bv0 34)) (_ bv3 34)) (_ bv0 34))
  (bvule (bvadd (bvadd elem (_ bv0 34)) (_ bv4 34)) list_end)
  (= (bvand (bvadd elem (_ bv4 34)) (_ bv3 34)) (_ bv0 34))
  (bvule (bvadd (bvadd elem (_ bv4 34)) (_ bv4 34)) list_end)
  (= (bvand (bvadd elem (_ bv8 34)) (_ bv3 34)) (_ bv0 34))
  (bvule (bvadd (bvadd elem (_ bv8 34)) (_ bv4 34)) list_end)
  (= (bvand (bvadd elem (_ bv12 34)) (_ bv3 34)) (_ bv0 34))
  (bvule (bvadd (bvadd elem (_ bv12 34)) (_ bv4 34)) list_end)))))
(check-sat)
