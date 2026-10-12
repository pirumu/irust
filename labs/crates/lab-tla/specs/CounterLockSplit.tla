-------------------------- MODULE CounterLockSplit --------------------------
(***************************************************************************)
(* Lab P13 (a). A lock with the wrong scope: the lock is released between  *)
(* the read and the write. Each step is still protected, so no two threads *)
(* are ever inside a critical section together (MutualExclusion holds),    *)
(* yet the lost update is back (Correct fails).                            *)
(*                                                                         *)
(* PlusCal (P-syntax) from the deep-dive of chapter 06, section 8.         *)
(* Translate first:                                                        *)
(*                                                                         *)
(*   java -cp tla2tools.jar pcal.trans -nocfg CounterLockSplit.tla         *)
(*                                                                         *)
(* then check with CounterLockSplit.cfg. Expected, NOT run: invariant      *)
(* Correct is violated; MutualExclusion holds.                             *)
(***************************************************************************)
EXTENDS Naturals

CONSTANT NumThreads
Threads == 1..NumThreads

(* --algorithm CounterLockSplit
variables counter = 0, lock = 0;

define
  AllDone == \A t \in Threads : pc[t] = "Done"
  Correct == AllDone => counter = NumThreads
  InCS(t) == pc[t] \in {"Load", "ReleaseR", "Store", "ReleaseW"}
  MutualExclusion ==
    \A t1, t2 \in Threads : t1 /= t2 => ~(InCS(t1) /\ InCS(t2))
end define;

process thread \in Threads
variable tmp = 0;
begin
  AcquireR: await lock = 0;
            lock := self;
  Load:     tmp := counter;
  ReleaseR: lock := 0;
  AcquireW: await lock = 0;
            lock := self;
  Store:    counter := tmp + 1;
  ReleaseW: lock := 0;
end process;
end algorithm; *)

\* BEGIN TRANSLATION
\* END TRANSLATION

TypeOK == /\ counter \in 0..NumThreads
          /\ lock \in 0..NumThreads
          /\ tmp \in [Threads -> 0..NumThreads]
          /\ pc \in [Threads -> {"AcquireR", "Load", "ReleaseR", "AcquireW",
                                 "Store", "ReleaseW", "Done"}]
=============================================================================
