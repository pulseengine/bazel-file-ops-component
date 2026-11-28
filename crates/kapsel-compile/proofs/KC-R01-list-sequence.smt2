; kapsel-compile status-logic obligation KC-R01 list-sequence (`a; b`: and-or lists in order)
; fixture: testdata/kapsel/compile/seq.plan.json
; implementation: 3 paths through the wasm `run` that compile_core emits; specification: 3 paths of the POSIX semantics
; inputs: cK_I = result of the I-th run of host call site K, nK_I = expand-list count; each site runs at most 3 times
; unsat: no host results make the two traces (call, expand-list, set-status, scope-push, scope-pop, return) differ
(set-logic QF_BV)
(declare-const c0_0 (_ BitVec 32))
(declare-const c1_0 (_ BitVec 32))
(assert (or
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (not (= (bvsub c0_0 (_ bv256 32)) (bvsub c0_0 (_ bv256 32)))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32)))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (or
  (not (= c0_0 c0_0))
  (not (= (bvsub c1_0 (_ bv256 32)) (bvsub c1_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32)))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (or
  (not (= c0_0 c0_0))
  (not (= c1_0 c1_0))
  (not (= c1_0 c1_0))))))
(check-sat)
