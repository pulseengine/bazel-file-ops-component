; kapsel-compile layout obligation: element offsets of a stride-16 list
; offset = 16 * i (layout lowering: base + STRIDE * i); for every i < n <= MAX_LIST = 65536
; 16 * x is written as a sum of shifts of x over the set bits of 16
; arithmetic at 22 bits: every input is below 2^32 and every intermediate below 2^21 (16 * 65536 + 16), so nothing wraps
(set-logic QF_BV)
(declare-const i (_ BitVec 22))
(declare-const n (_ BitVec 22))
(assert (and
  (bvule n (_ bv65536 22))
  (bvult i n)
  (not (and
  (= (bvand (bvshl i (_ bv4 22)) (_ bv3 22)) (_ bv0 22))
  (bvule (bvadd (bvshl i (_ bv4 22)) (_ bv16 22)) (bvshl n (_ bv4 22)))))))
(check-sat)
