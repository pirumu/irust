---------------------------- MODULE AtLeastOnce -----------------------------
(***************************************************************************)
(* Lab P13 (d). An at-least-once queue with an idempotent consumer.        *)
(* A broker may redeliver any in-flight message at any time (a visibility  *)
(* timeout collapsed into one action). A consumer receives a message,      *)
(* applies its side effect, then acknowledges, or "crashes" (the           *)
(* acknowledgement is lost and the message will be redelivered).           *)
(* Handle is ONE atomic step: the dedup check, the side effect and the     *)
(* dedup record belong to the same database transaction.                   *)
(*                                                                         *)
(* PlusCal (P-syntax) from chapter 09, section 3. Translate first:         *)
(*                                                                         *)
(*   java -cp tla2tools.jar pcal.trans -nocfg AtLeastOnce.tla              *)
(*                                                                         *)
(* Three configurations: AtLeastOnce.cfg (Dedup = TRUE, expected: no       *)
(* error), AtLeastOnceNoDedup.cfg and AtLeastOnceNoDedupCrash.cfg          *)
(* (Dedup = FALSE, expected: AtMostOneEffect violated). NOT run.           *)
(***************************************************************************)
EXTENDS Naturals

CONSTANT Msgs         \* the messages (model values)
CONSTANT Consumers    \* the consumers (model values)
CONSTANT NULL         \* "no message yet" (a model value)
CONSTANT Dedup        \* TRUE: skip the side effect for a message already in seen
CONSTANT MaxCrashes   \* budget of lost acknowledgements
ASSUME NULL \notin Msgs
ASSUME Dedup \in BOOLEAN
ASSUME MaxCrashes \in Nat

(*--algorithm at_least_once
variables
  status  = [m \in Msgs |-> "ready"],   \* at the broker: ready | inflight | acked
  effects = [m \in Msgs |-> 0],         \* side effects applied per message
  seen    = {},                         \* dedup table, in the same database as effects
  crashes = 0;                          \* budget used by lost acknowledgements

define
  \* The property the design needs.
  AtMostOneEffect == \A m \in Msgs : effects[m] <= 1
  \* The inductive invariant of the chapter: effects[m] = 1 exactly when m is in seen.
  EffectsIffSeen ==
    \A m \in Msgs : effects[m] <= 1 /\ ((effects[m] = 1) <=> (m \in seen))
end define;

process broker = "broker"
begin
  Redeliver:                            \* visibility timeout: inflight goes back to ready
    while TRUE do
      with m \in {x \in Msgs : status[x] = "inflight"} do
        status[m] := "ready";
      end with;
    end while;
end process;

process consumer \in Consumers
variable cur = NULL;
begin
  Receive:
    while TRUE do
      with m \in {x \in Msgs : status[x] = "ready"} do
        status[m] := "inflight";
        cur := m;
      end with;
  Handle:                               \* ONE transaction: effect + dedup record
      if ~(Dedup /\ cur \in seen) then
        effects[cur] := effects[cur] + 1;
        seen := seen \union {cur};
      end if;
  Ack:
      either
        status[cur] := "acked";
      or
        await crashes < MaxCrashes;     \* crash or lost ack: the broker will redeliver
        crashes := crashes + 1;
      end either;
    end while;
end process;
end algorithm; *)

\* BEGIN TRANSLATION
\* END TRANSLATION

TypeOK == /\ status \in [Msgs -> {"ready", "inflight", "acked"}]
          /\ \A m \in Msgs : effects[m] \in Nat
          /\ seen \subseteq Msgs
          /\ crashes \in 0..MaxCrashes
          /\ cur \in [Consumers -> Msgs \cup {NULL}]
=============================================================================
