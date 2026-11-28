; kapsel-compile layout obligation: allocator rounding to 4
; start = (len + a - 1) & !(a - 1), the formula of layout::align_up
; for every len <= 4294967295
; arithmetic at 34 bits: every input is below 2^32 and every intermediate below 2^33 (len + 3), so nothing wraps
(set-logic QF_BV)
(declare-const len (_ BitVec 34))
(assert (and
  (bvule len (_ bv4294967295 34))
  (not (and
  (= (bvand (bvand (bvadd len (_ bv3 34)) (_ bv17179869180 34)) (_ bv3 34)) (_ bv0 34))
  (bvule len (bvand (bvadd len (_ bv3 34)) (_ bv17179869180 34)))
  (bvult (bvsub (bvand (bvadd len (_ bv3 34)) (_ bv17179869180 34)) len) (_ bv4 34))))))
(check-sat)
