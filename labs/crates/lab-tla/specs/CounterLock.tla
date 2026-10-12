---------------------------- MODULE CounterLock ----------------------------
(***************************************************************************)
(* Lab P13 (a). The fix for CounterLost: one lock around the whole         *)
(* read-modify-write. Acquire checks the lock and takes it in one atomic   *)
(* step, like a compare-and-swap.                                          *)
(*                                                                         *)
(* PlusCal (P-syntax) from chapter 06, section 8, step 2. Translate first: *)
(*                                                                         *)
(*   java -cp tla2tools.jar pcal.trans -nocfg CounterLock.tla              *)
(*                                                                         *)
(* then check with CounterLock.cfg. Expected, NOT run: no error; Correct   *)
(* and MutualExclusion hold.                                               *)
(***************************************************************************)
EXTENDS Naturals

CONSTANT NumThreads
Threads == 1..NumThreads

(* --algorithm CounterLock
variables counter = 0, lock = 0;     \* lock = 0: free; lock = t: thread t holds it

define
  AllDone == \A t \in Threads : pc[t] = "Done"
  Correct == AllDone => counter = NumThreads
  InCS(t) == pc[t] \in {"Load", "Store", "Release"}
  MutualExclusion ==
    \A t1, t2 \in Threads : t1 /= t2 => ~(InCS(t1) /\ InCS(t2))
end define;

process thread \in Threads
variable tmp = 0;
begin
  Acquire: await lock = 0;
           lock := self;
  Load:    tmp := counter;
  Store:   counter := tmp + 1;
  Release: lock := 0;
end process;
end algorithm; *)

\* BEGIN TRANSLATION
\* END TRANSLATION

TypeOK == /\ counter \in 0..NumThreads
          /\ lock \in 0..NumThreads
          /\ tmp \in [Threads -> 0..NumThreads]
          /\ pc \in [Threads -> {"Acquire", "Load", "Store", "Release", "Done"}]
=============================================================================
