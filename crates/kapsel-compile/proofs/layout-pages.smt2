; kapsel-compile layout obligation: declared memory covers the data segment
; memory = pages_for(x) << 16, pages_for(x) = max(1, (x + 65535) >> 16), x = DATA_BASE + total <= 4294967295
; arithmetic at 34 bits: every input is below 2^32 and every intermediate below 2^33 (x + PAGE - 1, the memory size), so nothing wraps
(set-logic QF_BV)
(declare-const x (_ BitVec 34))
(assert (and
  (bvule x (_ bv4294967295 34))
  (not (and
  (bvule x (bvshl (ite (= (bvlshr (bvadd x (_ bv65535 34)) (_ bv16 34)) (_ bv0 34)) (_ bv1 34) (bvlshr (bvadd x (_ bv65535 34)) (_ bv16 34))) (_ bv16 34)))
  (bvule (bvshl (ite (= (bvlshr (bvadd x (_ bv65535 34)) (_ bv16 34)) (_ bv0 34)) (_ bv1 34) (bvlshr (bvadd x (_ bv65535 34)) (_ bv16 34))) (_ bv16 34)) (_ bv4294967296 34))
  (bvule (_ bv65536 34) (bvshl (ite (= (bvlshr (bvadd x (_ bv65535 34)) (_ bv16 34)) (_ bv0 34)) (_ bv1 34) (bvlshr (bvadd x (_ bv65535 34)) (_ bv16 34))) (_ bv16 34)))))))
(check-sat)
