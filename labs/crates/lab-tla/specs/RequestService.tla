-------------------------- MODULE RequestService ----------------------------
(***************************************************************************)
(* Lab P13 (e). "Every request that is pending is eventually done", under  *)
(* three levels of fairness. A request goes idle -> pending -> done; a     *)
(* worker serves it, and the worker can crash and restart.                 *)
(*                                                                         *)
(* Plain TLA+ from chapter 05, section 6. Not run through TLC by the       *)
(* author. Three configurations, one per spec:                             *)
(*   RequestServiceNoFair.cfg  Spec0  expected: AllServed violated         *)
(*                                    (stuttering after Submit)            *)
(*   RequestServiceWF.cfg      Spec1  expected: AllServed violated         *)
(*                                    (a lasso: Crash, Restart, ...)       *)
(*   RequestServiceSF.cfg      Spec2  expected: no error                   *)
(***************************************************************************)
CONSTANT Reqs                  \* the requests that can appear (model values)
VARIABLES req, up              \* req[r] \in {"idle","pending","done"}; up: worker alive?

vars == <<req, up>>

TypeOK == /\ req \in [Reqs -> {"idle", "pending", "done"}]
          /\ up \in BOOLEAN

Init == /\ req = [r \in Reqs |-> "idle"]
        /\ up = TRUE

Submit(r)  == /\ req[r] = "idle"
              /\ req' = [req EXCEPT ![r] = "pending"]
              /\ UNCHANGED up

Process(r) == /\ up
              /\ req[r] = "pending"
              /\ req' = [req EXCEPT ![r] = "done"]
              /\ UNCHANGED up

Crash   == /\ up
           /\ up' = FALSE
           /\ UNCHANGED req

Restart == /\ ~up
           /\ up' = TRUE
           /\ UNCHANGED req

Next == \/ \E r \in Reqs : (Submit(r) \/ Process(r))
        \/ Crash
        \/ Restart

\* Three fairness levels for the same Init /\ [][Next]_vars.
Spec0 == Init /\ [][Next]_vars

Fair1 == /\ WF_vars(Restart)
         /\ \A r \in Reqs : WF_vars(Process(r))
Spec1 == Init /\ [][Next]_vars /\ Fair1

Fair2 == /\ WF_vars(Restart)
         /\ \A r \in Reqs : SF_vars(Process(r))
Spec2 == Init /\ [][Next]_vars /\ Fair2

\* Liveness: a request that is pending is eventually done.
AllServed == \A r \in Reqs : (req[r] = "pending") ~> (req[r] = "done")
=============================================================================
