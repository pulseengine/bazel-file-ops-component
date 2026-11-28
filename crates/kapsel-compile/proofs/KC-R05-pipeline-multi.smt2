; kapsel-compile status-logic obligation KC-R05 pipeline-multi (pipeline of N >= 2 stages: N-1 pipes)
; fixture: testdata/kapsel/compile/pipe3.plan.json
; implementation: 8 paths through the wasm `run` that compile_core emits; specification: 8 paths of the POSIX semantics
; inputs: cK_I = result of the I-th run of host call site K, nK_I = expand-list count; each site runs at most 3 times
; unsat: no host results make the two traces (call, expand-list, set-status, scope-push, scope-pop, return) differ
(set-logic QF_BV)
(declare-const c0_0 (_ BitVec 32))
(declare-const c1_0 (_ BitVec 32))
(declare-const c2_0 (_ BitVec 32))
(assert (or
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))
  (not (= (bvsub c2_0 (_ bv256 32)) (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))
  (not (= (bvsub c2_0 (_ bv256 32)) c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (not (bvult c0_0 (_ bv256 32)))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (not (bvult c1_0 (_ bv256 32)))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (not (bvult c2_0 (_ bv256 32)))
  (or
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))
  (not (= c2_0 (bvsub c2_0 (_ bv256 32))))))
  (and
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (bvult c0_0 (_ bv256 32))
  (bvult c1_0 (_ bv256 32))
  (bvult c2_0 (_ bv256 32))
  (or
  (not (= c2_0 c2_0))
  (not (= c2_0 c2_0))))))
(check-sat)
