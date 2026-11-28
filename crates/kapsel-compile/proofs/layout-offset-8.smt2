; kapsel-compile layout obligation: element offsets of a stride-8 list
; offset = 8 * i (layout lowering: base + STRIDE * i); for every i < n <= MAX_LIST = 65536
; 8 * x is written as a sum of shifts of x over the set bits of 8
; arithmetic at 22 bits: every input is below 2^32 and every intermediate below 2^21 (8 * 65536 + 8), so nothing wraps
(set-logic QF_BV)
(declare-const i (_ BitVec 22))
(declare-const n (_ BitVec 22))
(assert (and
  (bvule n (_ bv65536 22))
  (bvult i n)
  (not (and
  (= (bvand (bvshl i (_ bv3 22)) (_ bv3 22)) (_ bv0 22))
  (bvule (bvadd (bvshl i (_ bv3 22)) (_ bv8 22)) (bvshl n (_ bv3 22)))))))
(check-sat)
