---------------------------- MODULE CounterLost ----------------------------
(***************************************************************************)
(* Lab P13 (a). Threads each run  counter := counter + 1  as TWO atomic    *)
(* steps: read the counter into a local variable, then write local + 1.    *)
(* Another thread can run between the two steps, so an update can be lost. *)
(*                                                                         *)
(* This is PlusCal (P-syntax) from chapter 06, section 8, step 1.          *)
(* Translate first; the translator fills in the region between the two     *)
(* translation marker comments below:                                      *)
(*                                                                         *)
(*   java -cp tla2tools.jar pcal.trans -nocfg CounterLost.tla              *)
(*                                                                         *)
(* then check with CounterLost.cfg. Expected, NOT run: TLC reports that    *)
(* invariant Correct is violated.                                          *)
(***************************************************************************)
EXTENDS Naturals

CONSTANT NumThreads
Threads == 1..NumThreads

(* --algorithm Counter
variables counter = 0;

define
  AllDone == \A t \in Threads : pc[t] = "Done"
  Correct == AllDone => counter = NumThreads
end define;

process thread \in Threads
variable tmp = 0;
begin
  Load:  tmp := counter;
  Store: counter := tmp + 1;
end process;
end algorithm; *)

\* BEGIN TRANSLATION
\* END TRANSLATION

\* Type invariant. It uses the local variable tmp, which `define` cannot see,
\* so it has to come after the translation.
TypeOK == /\ counter \in 0..NumThreads
          /\ tmp \in [Threads -> 0..NumThreads]
          /\ pc \in [Threads -> {"Load", "Store", "Done"}]
=============================================================================
