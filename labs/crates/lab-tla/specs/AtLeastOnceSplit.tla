-------------------------- MODULE AtLeastOnceSplit --------------------------
(***************************************************************************)
(* Lab P13 (d). Same queue as AtLeastOnce.tla, with one change: the side   *)
(* effect and the dedup record are TWO atomic steps (Effect, then Record), *)
(* not one database transaction. Dedup is on, yet a second consumer can    *)
(* run Effect between the first consumer's Effect and Record.              *)
(*                                                                         *)
(* PlusCal (P-syntax), the "split" variant of chapter 09, section 3.       *)
(* Translate first:                                                        *)
(*                                                                         *)
(*   java -cp tla2tools.jar pcal.trans -nocfg AtLeastOnceSplit.tla         *)
(*                                                                         *)
(* then check with AtLeastOnceSplit.cfg. Expected, NOT run: invariant      *)
(* AtMostOneEffect is violated although Dedup = TRUE and no crash occurs.  *)
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

(*--algorithm at_least_once_split
variables
  status  = [m \in Msgs |-> "ready"],
  effects = [m \in Msgs |-> 0],
  seen    = {},
  crashes = 0;

define
  AtMostOneEffect == \A m \in Msgs : effects[m] <= 1
  EffectsIffSeen ==
    \A m \in Msgs : effects[m] <= 1 /\ ((effects[m] = 1) <=> (m \in seen))
end define;

process broker = "broker"
begin
  Redeliver:
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
  Effect:
      if ~(Dedup /\ cur \in seen) then
        effects[cur] := effects[cur] + 1;
      end if;
  Record:
      seen := seen \union {cur};
  Ack:
      either
        status[cur] := "acked";
      or
        await crashes < MaxCrashes;
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
