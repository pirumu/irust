------------------------------- MODULE TCommit ------------------------------
(***************************************************************************)
(* Lab P13 (c). The Transaction Commit requirement of Gray and Lamport:    *)
(* resource managers (RMs) move working -> prepared -> committed/aborted,  *)
(* and no RM may commit while another has aborted.                         *)
(*                                                                         *)
(* Written after specifications/transaction_commit/TCommit.tla of          *)
(* tlaplus/Examples (chapter 08, section 2). One addition: the constant    *)
(* Buggy. With Buggy = TRUE the abort branch of Decide forgets             *)
(* notCommitted, a deliberate bug that TCConsistent must catch.            *)
(* Not run through TLC by the author.                                      *)
(***************************************************************************)
CONSTANT RM       \* The set of participating resource managers
CONSTANT Buggy    \* FALSE: the specification. TRUE: abort without notCommitted.
ASSUME Buggy \in BOOLEAN

VARIABLE rmState  \* rmState[rm] is the state of resource manager rm.
-----------------------------------------------------------------------------
TCTypeOK ==
  rmState \in [RM -> {"working", "prepared", "committed", "aborted"}]

TCInit == rmState = [rm \in RM |-> "working"]

canCommit == \A rm \in RM : rmState[rm] \in {"prepared", "committed"}

notCommitted == \A rm \in RM : rmState[rm] # "committed"
-----------------------------------------------------------------------------
Prepare(rm) == /\ rmState[rm] = "working"
               /\ rmState' = [rmState EXCEPT ![rm] = "prepared"]

Decide(rm)  == \/ /\ rmState[rm] = "prepared"
                  /\ canCommit
                  /\ rmState' = [rmState EXCEPT ![rm] = "committed"]
               \/ /\ rmState[rm] \in {"working", "prepared"}
                  /\ (Buggy \/ notCommitted)
                  /\ rmState' = [rmState EXCEPT ![rm] = "aborted"]

TCNext == \E rm \in RM : Prepare(rm) \/ Decide(rm)
-----------------------------------------------------------------------------
TCSpec == TCInit /\ [][TCNext]_rmState
-----------------------------------------------------------------------------
\* Two RMs have not arrived at conflicting decisions.
TCConsistent ==
  \A rm1, rm2 \in RM : ~ /\ rmState[rm1] = "aborted"
                         /\ rmState[rm2] = "committed"
=============================================================================
