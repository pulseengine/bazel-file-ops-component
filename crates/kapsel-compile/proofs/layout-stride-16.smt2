; kapsel-compile layout obligation: list elements of stride 16
; elem = base + offset, list = [base, base + size); base 4-aligned (layout-bounds), the list below 2^32
; hypotheses from layout-offset-16: offset 4-aligned, offset + 16 <= size, size = 16 * n <= 16 * MAX_LIST
; arithmetic at 34 bits: every input is below 2^32 and every intermediate below 2^34 (base + size + s), so nothing wraps
(set-logic QF_BV)
(declare-const base (_ BitVec 34))
(declare-const offset (_ BitVec 34))
(declare-const size (_ BitVec 34))
(assert (and
  (= (bvand base (_ bv3 34)) (_ bv0 34))
  (bvule base (_ bv4294967296 34))
  (= (bvand offset (_ bv3 34)) (_ bv0 34))
  (bvule (bvadd offset (_ bv16 34)) size)
  (bvule size (_ bv1048576 34))
  (bvule (bvadd base size) (_ bv4294967296 34))
  (not (and
  (= (bvand (bvadd base offset) (_ bv3 34)) (_ bv0 34))
  (bvule (bvadd (bvadd base offset) (_ bv16 34)) (bvadd base size))))))
(check-sat)
