------------------------------- MODULE TwoPhase -----------------------------
(***************************************************************************)
(* Lab P13 (c). The Two-Phase Commit protocol: a transaction manager (TM)  *)
(* coordinates the resource managers (RMs) through a set msgs of every     *)
(* message ever sent (messages are never removed, so a receipt can repeat  *)
(* and a message can be ignored).                                          *)
(*                                                                         *)
(* Written after specifications/transaction_commit/TwoPhase.tla of         *)
(* tlaplus/Examples (chapter 08, section 3). Additions: the constant       *)
(* Buggy (TRUE: TMCommit no longer waits for every Prepared message), the  *)
(* definition vars, and two definitions for TLC: TPConsistent and          *)
(* RefinesTCommit. TCommit.tla must be in the same directory.              *)
(* Not run through TLC by the author.                                      *)
(***************************************************************************)
CONSTANT RM       \* The set of resource managers
CONSTANT Buggy    \* FALSE: the specification. TRUE: the TM commits too early.
ASSUME Buggy \in BOOLEAN

VARIABLES
  rmState,       \* rmState[rm] is the state of resource manager rm.
  tmState,       \* The state of the transaction manager.
  tmPrepared,    \* The set of RMs from which the TM has received "Prepared" messages.
  msgs           \* The set of all messages that have been sent.

vars == <<rmState, tmState, tmPrepared, msgs>>

Message ==
  [type : {"Prepared"}, rm : RM]  \cup  [type : {"Commit", "Abort"}]

TPTypeOK ==
  /\ rmState \in [RM -> {"working", "prepared", "committed", "aborted"}]
  /\ tmState \in {"init", "committed", "aborted"}
  /\ tmPrepared \subseteq RM
  /\ msgs \subseteq Message

TPInit ==
  /\ rmState = [rm \in RM |-> "working"]
  /\ tmState = "init"
  /\ tmPrepared = {}
  /\ msgs = {}
-----------------------------------------------------------------------------
\* The TM receives a "Prepared" message from resource manager rm.
TMRcvPrepared(rm) ==
  /\ tmState = "init"
  /\ [type |-> "Prepared", rm |-> rm] \in msgs
  /\ tmPrepared' = tmPrepared \cup {rm}
  /\ UNCHANGED <<rmState, tmState, msgs>>

\* The TM commits the transaction; enabled iff the TM is in its initial state and
\* every RM has sent a "Prepared" message (unless Buggy).
TMCommit ==
  /\ tmState = "init"
  /\ (Buggy \/ tmPrepared = RM)
  /\ tmState' = "committed"
  /\ msgs' = msgs \cup {[type |-> "Commit"]}
  /\ UNCHANGED <<rmState, tmPrepared>>

\* The TM spontaneously aborts the transaction.
TMAbort ==
  /\ tmState = "init"
  /\ tmState' = "aborted"
  /\ msgs' = msgs \cup {[type |-> "Abort"]}
  /\ UNCHANGED <<rmState, tmPrepared>>

\* Resource manager rm prepares.
RMPrepare(rm) ==
  /\ rmState[rm] = "working"
  /\ rmState' = [rmState EXCEPT ![rm] = "prepared"]
  /\ msgs' = msgs \cup {[type |-> "Prepared", rm |-> rm]}
  /\ UNCHANGED <<tmState, tmPrepared>>

\* Resource manager rm spontaneously decides to abort; it sends no message.
RMChooseToAbort(rm) ==
  /\ rmState[rm] = "working"
  /\ rmState' = [rmState EXCEPT ![rm] = "aborted"]
  /\ UNCHANGED <<tmState, tmPrepared, msgs>>

\* Resource manager rm is told by the TM to commit.
RMRcvCommitMsg(rm) ==
  /\ [type |-> "Commit"] \in msgs
  /\ rmState' = [rmState EXCEPT ![rm] = "committed"]
  /\ UNCHANGED <<tmState, tmPrepared, msgs>>

\* Resource manager rm is told by the TM to abort.
RMRcvAbortMsg(rm) ==
  /\ [type |-> "Abort"] \in msgs
  /\ rmState' = [rmState EXCEPT ![rm] = "aborted"]
  /\ UNCHANGED <<tmState, tmPrepared, msgs>>

TPNext ==
  \/ TMCommit \/ TMAbort
  \/ \E rm \in RM :
       TMRcvPrepared(rm) \/ RMPrepare(rm) \/ RMChooseToAbort(rm)
         \/ RMRcvCommitMsg(rm) \/ RMRcvAbortMsg(rm)
-----------------------------------------------------------------------------
TPSpec == TPInit /\ [][TPNext]_vars
-----------------------------------------------------------------------------
\* TCommit.tla, instantiated with Buggy <- FALSE: TC!TCSpec is the requirement.
TC == INSTANCE TCommit WITH Buggy <- FALSE

\* The invariant of the requirement, checked on TwoPhase states.
TPConsistent == TC!TCConsistent

\* "TwoPhase implements TCommit". Declare it with PROPERTY: TLC then checks, on every step,
\* that the step is a TCommit step or leaves rmState unchanged.
RefinesTCommit == TC!TCSpec
=============================================================================
