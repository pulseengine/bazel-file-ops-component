; kapsel-compile layout obligation: `redirect` fields (stride 20, align 4)
; fields: fd@0:4/4, op@4:1/1, target.ptr@8:4/4, target.len@12:4/4, target-fd@16:4/4
; for every element start aligned to 4 with start + 20 <= list end <= 2^32 (layout-stride-20)
; arithmetic at 34 bits: every input is below 2^32 and every intermediate below 2^33 (elem + offset + size), so nothing wraps
(set-logic QF_BV)
(declare-const elem (_ BitVec 34))
(declare-const list_end (_ BitVec 34))
(assert (and
  (= (bvand elem (_ bv3 34)) (_ bv0 34))
  (bvule elem (_ bv4294967296 34))
  (bvule list_end (_ bv4294967296 34))
  (bvule (bvadd elem (_ bv20 34)) list_end)
  (not (and
  (= (bvand (bvadd elem (_ bv0 34)) (_ bv3 34)) (_ bv0 34))
  (bvule (bvadd (bvadd elem (_ bv0 34)) (_ bv4 34)) list_end)
  (= (bvand (bvadd elem (_ bv4 34)) (_ bv0 34)) (_ bv0 34))
  (bvule (bvadd (bvadd elem (_ bv4 34)) (_ bv1 34)) list_end)
  (= (bvand (bvadd elem (_ bv8 34)) (_ bv3 34)) (_ bv0 34))
  (bvule (bvadd (bvadd elem (_ bv8 34)) (_ bv4 34)) list_end)
  (= (bvand (bvadd elem (_ bv12 34)) (_ bv3 34)) (_ bv0 34))
  (bvule (bvadd (bvadd elem (_ bv12 34)) (_ bv4 34)) list_end)
  (= (bvand (bvadd elem (_ bv16 34)) (_ bv3 34)) (_ bv0 34))
  (bvule (bvadd (bvadd elem (_ bv16 34)) (_ bv4 34)) list_end)))))
(check-sat)
