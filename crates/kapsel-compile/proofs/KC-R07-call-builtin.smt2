; kapsel-compile status-logic obligation KC-R07 call-builtin (built-in call with a literal argv[0])
; fixture: testdata/kapsel/compile/redirects.plan.json
; implementation: 2 paths through the wasm `run` that compile_core emits; specification: 2 paths of the POSIX semantics
; inputs: cK_I = result of the I-th run of host call site K, nK_I = expand-list count; each site runs at most 3 times
; unsat: no host results make the two traces (call, expand-list, set-status, scope-push, scope-pop, return) differ
(set-logic QF_BV)
(declare-const c0_0 (_ BitVec 32))
(assert (or
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (not (= (bvsub c0_0 (_ bv256 32)) (bvsub c0_0 (_ bv256 32)))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32)))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (or
  (not (= c0_0 c0_0))
  (not (= c0_0 c0_0))))))
(check-sat)
