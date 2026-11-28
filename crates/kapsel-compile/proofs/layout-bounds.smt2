; kapsel-compile layout obligation: allocations stay inside the data segment
; ptr = 16 + start (start = align_up(len, a), aligned by layout-align-*); accepted when 16 + start + size <= 4294967295 (the KC-003 check)
; total = final segment length >= start + size; segment = [16, 16 + total)
; with layout-pages: ptr + size <= DATA_BASE + total <= memory size
; arithmetic at 34 bits: every input is below 2^32 and every intermediate below 2^34 (DATA_BASE + start + size), so nothing wraps
(set-logic QF_BV)
(declare-const size (_ BitVec 34))
(declare-const start (_ BitVec 34))
(declare-const total (_ BitVec 34))
(assert (and
  (bvule start (_ bv4294967295 34))
  (bvule size (_ bv4294967295 34))
  (bvule total (_ bv4294967295 34))
  (bvule (bvadd (_ bv16 34) (bvadd start size)) (_ bv4294967295 34))
  (bvule (bvadd start size) total)
  (not (and
  (bvule (bvadd (bvadd (_ bv16 34) start) size) (bvadd (_ bv16 34) total))
  (bvule (bvadd (bvadd (_ bv16 34) start) size) (_ bv4294967296 34))
  (or
  (not (= (bvand start (_ bv3 34)) (_ bv0 34)))
  (= (bvand (bvadd (_ bv16 34) start) (_ bv3 34)) (_ bv0 34)))
  (= (bvand (_ bv8 34) (_ bv3 34)) (_ bv0 34))))))
(check-sat)
