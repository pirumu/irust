//! Lab (h): event time vs processing time, tumbling/hopping windows, bounded-out-of-orderness
//! watermarks, late events (drop / allowed lateness) and idle sources.
//!
//! The model is a *pure function of a seed*. All times are integer milliseconds on one shared
//! virtual axis: the event-time axis and the processing-time axis use the same origin and tick at
//! the same rate (no clock skew; the only difference between the two domains is the per-event
//! network/queue delay `arrival - ts`). Windows are tumbling `[s, s + W)`.
//!
//! The watermark is the simple heuristic `max_event_ts_seen - D` ("bounded out-of-orderness"),
//! evaluated per input partition, with the operator watermark being the minimum over the
//! partitions that are not idle. It is *not* what a production system must use: it needs the
//! designer to know a bound `D` on the lateness, which real systems usually do not.

use crate::rng::Rng;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// Sentinel for "no watermark yet" (kept far from `i64::MIN` so arithmetic never overflows).
const NEG: i64 = i64::MIN / 2;

/// Latency statistics only cover windows ending at or before this event time (ms): with the
/// default 2000 events (~14 s) these windows fire on a watermark for every `D <= 3000`.
pub const LATENCY_END_LIMIT: u64 = 10_000;

/// One input event: true event time `ts`, plus the processing time at which it reaches the
/// pipeline (`arrival >= ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// Sequence number in event-time order (ground truth).
    pub id: u64,
    /// Grouping key (only used to derive `part`; counts in this lab are per window).
    pub key: u32,
    /// Input partition (source) the event is read from.
    pub part: usize,
    /// Event time in ms: when the event really happened.
    pub ts: u64,
    /// Processing time in ms: when the pipeline sees it.
    pub arrival: u64,
}

/// Seeded delay distribution, in ms: with probability `1 - mid - tail` a small delay in
/// `[0, small_max)`, with `mid_permille` a medium one in `[small_max, mid_hi)`, with
/// `tail_permille` a heavy-tail one in `[tail_lo, tail_hi)` ("the shipper in the tunnel").
#[derive(Clone, Copy, Debug)]
pub struct DelayModel {
    pub small_max: u64,
    pub mid_permille: u64,
    pub mid_hi: u64,
    pub tail_permille: u64,
    pub tail_lo: u64,
    pub tail_hi: u64,
}

impl DelayModel {
    /// The lab's default: 85% in `[0,40)`, 12% in `[40,400)`, 3% in `[1000,3000)`.
    pub const HEAVY_TAIL: DelayModel = DelayModel {
        small_max: 40,
        mid_permille: 120,
        mid_hi: 400,
        tail_permille: 30,
        tail_lo: 1000,
        tail_hi: 3000,
    };
    /// Only small delays in `[0, 40)` (bounded, no tail).
    pub const SMALL: DelayModel = DelayModel {
        small_max: 40,
        mid_permille: 0,
        mid_hi: 0,
        tail_permille: 0,
        tail_lo: 0,
        tail_hi: 0,
    };
    /// Every event arrives at its event time (no out-of-orderness at all).
    pub const NONE: DelayModel = DelayModel {
        small_max: 1,
        mid_permille: 0,
        mid_hi: 0,
        tail_permille: 0,
        tail_lo: 0,
        tail_hi: 0,
    };

    /// Largest delay this model can ever draw.
    pub fn max_delay(&self) -> u64 {
        if self.tail_permille > 0 {
            self.tail_hi - 1
        } else if self.mid_permille > 0 {
            self.mid_hi - 1
        } else {
            self.small_max - 1
        }
    }

    fn sample(&self, rng: &mut Rng) -> u64 {
        let x = rng.below(1000);
        if x < self.tail_permille {
            rng.range(self.tail_lo, self.tail_hi)
        } else if x < self.tail_permille + self.mid_permille {
            rng.range(self.small_max, self.mid_hi)
        } else {
            rng.below(self.small_max)
        }
    }
}

/// Number of keys; the partition of an event is `key % 2`.
pub const KEYS: u32 = 4;

/// Generates `n` events, one per ~7 ms of event time, with delays drawn from `delay`.
/// The result is sorted in *arrival order* (ties by id) - the order the pipeline sees them.
pub fn generate(seed: u64, n: usize, delay: &DelayModel) -> Vec<Event> {
    let mut root = Rng::new(seed);
    let mut r_ts = root.fork(1);
    let mut r_delay = root.fork(2);
    let mut events: Vec<Event> = (0..n as u64)
        .map(|id| {
            let ts = id * 7 + r_ts.below(7);
            let key = r_ts.below(u64::from(KEYS)) as u32;
            let d = delay.sample(&mut r_delay);
            Event {
                id,
                key,
                part: (key % 2) as usize,
                ts,
                arrival: ts + d,
            }
        })
        .collect();
    events.sort_by_key(|e| (e.arrival, e.id));
    events
}

/// Turns a partition's data stream into an outage with a drain: every event of `part` with
/// `ts >= from` is delivered no earlier than `to + (ts - from) / 20`, i.e. the backlog is drained in
/// event-time order at 20 ms of event time per ms of processing time (events inside `[from, to)`
/// are held until `to`; later events of that partition queue behind the backlog). Returns the
/// events re-sorted in arrival order.
pub fn with_outage(mut events: Vec<Event>, part: usize, from: u64, to: u64) -> Vec<Event> {
    for e in events.iter_mut().filter(|e| e.part == part && e.ts >= from) {
        e.arrival = e.arrival.max(to + (e.ts - from) / 20);
    }
    events.sort_by_key(|e| (e.arrival, e.id));
    events
}

/// Largest `arrival - ts` over the events.
pub fn observed_max_delay(events: &[Event]) -> u64 {
    events.iter().map(|e| e.arrival - e.ts).max().unwrap_or(0)
}

/// Start of the tumbling window `[s, s + w)` containing `ts`.
pub fn tumbling_start(ts: u64, w: u64) -> u64 {
    ts / w * w
}

/// Number of events whose arrival window differs from their event-time window.
pub fn crossing_events(events: &[Event], w: u64) -> usize {
    events
        .iter()
        .filter(|e| tumbling_start(e.ts, w) != tumbling_start(e.arrival, w))
        .count()
}

/// Ground truth: events per tumbling window by *event time* (window start -> count).
pub fn truth_counts(events: &[Event], w: u64) -> BTreeMap<u64, u64> {
    let mut m = BTreeMap::new();
    for e in events {
        *m.entry(tumbling_start(e.ts, w)).or_insert(0) += 1;
    }
    m
}

/// Processing-time windows: events per tumbling window by *arrival* time. These close on the
/// clock, so there are no late events and the latency is 0 by construction.
pub fn processing_time_counts(events: &[Event], w: u64) -> BTreeMap<u64, u64> {
    let mut m = BTreeMap::new();
    for e in events {
        *m.entry(tumbling_start(e.arrival, w)).or_insert(0) += 1;
    }
    m
}

/// Windows (union of both key sets; missing = 0) whose count differs between `got` and `truth`.
pub fn wrong_windows(truth: &BTreeMap<u64, u64>, got: &BTreeMap<u64, u64>) -> usize {
    let keys: BTreeSet<u64> = truth.keys().chain(got.keys()).copied().collect();
    keys.iter()
        .filter(|k| truth.get(k).unwrap_or(&0) != got.get(k).unwrap_or(&0))
        .count()
}

/// Starts of the hopping windows `[s, s + size)` with `s = k * slide` that contain `ts`.
/// Windows are allowed to start before the epoch (negative `s`), so every event lands in exactly
/// `size / slide` windows when `slide` divides `size`.
pub fn hopping_starts(ts: u64, size: u64, slide: u64) -> Vec<i64> {
    let ts = ts as i64;
    let (size, slide) = (size as i64, slide as i64);
    // Last window start <= ts, then walk back while the window still contains ts.
    let mut s = ts.div_euclid(slide) * slide;
    let mut out = Vec::new();
    while s > ts - size {
        out.push(s);
        s -= slide;
    }
    out.reverse();
    out
}

/// What to do with an event whose window has already fired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatePolicy {
    /// Allowed lateness 0: the event is dropped (and counted).
    Drop,
    /// While `wm < end + L` the late event is added and the window fires an updated result;
    /// later events are dropped.
    AllowedLateness(u64),
}

impl LatePolicy {
    fn lateness(self) -> u64 {
        match self {
            LatePolicy::Drop => 0,
            LatePolicy::AllowedLateness(l) => l,
        }
    }
}

/// Parameters of one event-time run.
#[derive(Clone, Copy, Debug)]
pub struct RunConfig {
    /// Tumbling window size W in ms.
    pub window: u64,
    /// Watermark delay D in ms: `wm = max_ts_seen - D`.
    pub out_of_orderness: u64,
    pub policy: LatePolicy,
    /// Number of input partitions (`event.part % partitions` selects the source).
    pub partitions: usize,
    /// A partition silent for this long (processing time) is excluded from the min-watermark.
    pub idle_timeout: Option<u64>,
}

impl RunConfig {
    /// One partition, no idle detection.
    pub fn simple(window: u64, d: u64, policy: LatePolicy) -> Self {
        RunConfig {
            window,
            out_of_orderness: d,
            policy,
            partitions: 1,
            idle_timeout: None,
        }
    }
}

/// One emitted window result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Firing {
    pub window_start: u64,
    pub count: u64,
    /// Processing time at which the result was emitted.
    pub fire_at: u64,
    /// `true` for an updated result of an already-fired window.
    pub refire: bool,
}

/// Result of [`run_event_time`].
#[derive(Clone, Debug)]
pub struct Run {
    pub firings: Vec<Firing>,
    /// Final count of every window that received at least one event.
    pub final_counts: BTreeMap<u64, u64>,
    /// Events dropped because their window had been purged (late beyond allowed lateness).
    pub dropped: u64,
    /// Events that arrived after the watermark had passed the end of their window, i.e. after the window
    /// had fired (dropped, or absorbed by allowed lateness). Narrower than Flink's per-timestamp notion of a late element.
    pub late: u64,
    /// Windows that never fired on a watermark and were fired by the end-of-input flush.
    pub flushed: usize,
    /// Operator watermark after every event, in arrival order.
    pub wm_trace: Vec<i64>,
    /// Window width (to compute latencies).
    pub window: u64,
}

/// Latency statistics in ms (processing-time fire time minus event-time window end).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Latency {
    /// Mean/max over windows of the *first* result.
    pub first_mean: u64,
    pub first_max: u64,
    /// Mean/max over windows of the time of the *last* result (when the answer is final).
    pub final_mean: u64,
    pub final_max: u64,
}

impl Run {
    /// Number of re-fires (updated results of already-fired windows).
    pub fn refires(&self) -> usize {
        self.firings.iter().filter(|f| f.refire).count()
    }

    /// First and final result latency, integer ms (negative values saturate to 0), over the
    /// windows with `end <= end_limit`. The limit keeps the statistic independent of the
    /// end-of-input flush: the last windows of a bounded input fire only when the input ends.
    pub fn latency(&self, end_limit: u64) -> Latency {
        let mut first: BTreeMap<u64, u64> = BTreeMap::new();
        let mut last: BTreeMap<u64, u64> = BTreeMap::new();
        for f in self
            .firings
            .iter()
            .filter(|f| f.window_start + self.window <= end_limit)
        {
            let end = f.window_start + self.window;
            let lat = f.fire_at.saturating_sub(end);
            first.entry(f.window_start).or_insert(lat);
            last.insert(f.window_start, lat);
        }
        let n = first.len().max(1) as u64;
        Latency {
            first_mean: first.values().sum::<u64>() / n,
            first_max: first.values().copied().max().unwrap_or(0),
            final_mean: last.values().sum::<u64>() / n,
            final_max: last.values().copied().max().unwrap_or(0),
        }
    }

    /// Number of windows whose first result was emitted strictly before processing time `t`.
    pub fn first_fired_before(&self, t: u64) -> usize {
        self.firings
            .iter()
            .filter(|f| !f.refire && f.fire_at < t)
            .count()
    }

    /// Count carried by the last firing of each window (what a downstream consumer ends with).
    pub fn last_firing_counts(&self) -> BTreeMap<u64, u64> {
        let mut m = BTreeMap::new();
        for f in &self.firings {
            m.insert(f.window_start, f.count);
        }
        m
    }
}

/// Runs the event-time pipeline over `events` (which must be in arrival order).
///
/// Per event: (1) lateness check against the operator watermark *before* the event, (2) add it
/// to its window (or drop it), (3) advance the partition watermark, recompute the operator
/// watermark = min over active partitions (never moving backwards) and fire every window with
/// `end <= wm`. At end of input the watermark jumps to infinity and the remaining windows fire.
pub fn run_event_time(events: &[Event], cfg: &RunConfig) -> Run {
    let w = cfg.window;
    let lateness = cfg.policy.lateness();
    let mut counts: BTreeMap<u64, u64> = BTreeMap::new();
    let mut fired: BTreeSet<u64> = BTreeSet::new();
    let mut part_wm: Vec<i64> = vec![NEG; cfg.partitions];
    let mut last_seen: Vec<u64> = vec![0; cfg.partitions];
    let mut wm_out = NEG;
    let mut run = Run {
        firings: Vec::new(),
        final_counts: BTreeMap::new(),
        dropped: 0,
        late: 0,
        flushed: 0,
        wm_trace: Vec::with_capacity(events.len()),
        window: w,
    };

    for e in events {
        let p = e.part % cfg.partitions;
        let s = tumbling_start(e.ts, w);
        let end = (s + w) as i64;
        if wm_out >= end + lateness as i64 {
            // The window state is gone: late beyond allowed lateness.
            run.dropped += 1;
            run.late += 1;
        } else {
            let c = counts.entry(s).or_insert(0);
            *c += 1;
            if wm_out >= end {
                run.late += 1;
                run.firings.push(Firing {
                    window_start: s,
                    count: *c,
                    fire_at: e.arrival,
                    refire: !fired.insert(s),
                });
            }
        }

        part_wm[p] = part_wm[p].max(e.ts as i64 - cfg.out_of_orderness as i64);
        last_seen[p] = e.arrival;
        let mut cand = i64::MAX;
        for (i, wm) in part_wm.iter().enumerate() {
            let idle = cfg
                .idle_timeout
                .is_some_and(|t| e.arrival.saturating_sub(last_seen[i]) >= t);
            if !idle {
                cand = cand.min(*wm);
            }
        }
        if cand > wm_out {
            wm_out = cand;
            for (&s, &c) in &counts {
                if !fired.contains(&s) && (s + w) as i64 <= wm_out {
                    fired.insert(s);
                    run.firings.push(Firing {
                        window_start: s,
                        count: c,
                        fire_at: e.arrival,
                        refire: false,
                    });
                }
            }
        }
        run.wm_trace.push(wm_out);
    }

    // End of input: watermark = +infinity, everything left fires.
    let last_arrival = events.last().map_or(0, |e| e.arrival);
    for (&s, &c) in &counts {
        if fired.insert(s) {
            run.flushed += 1;
            run.firings.push(Firing {
                window_start: s,
                count: c,
                fire_at: last_arrival,
                refire: false,
            });
        }
    }
    run.final_counts = counts;
    run
}

/// Builds the text printed by the `watermark` binary (a pure function of `seed`).
pub fn report(seed: u64) -> String {
    const E: usize = 2000;
    const W: u64 = 1000;
    let dm = DelayModel::HEAVY_TAIL;
    let events = generate(seed, E, &dm);
    let truth = truth_counts(&events, W);
    let proc = processing_time_counts(&events, W);
    let maxd = observed_max_delay(&events);
    let mut o = String::new();
    let _ = writeln!(
        o,
        "== Lab h: event time vs processing time, watermark (seed {seed}) =="
    );
    let _ = writeln!(
        o,
        "E={E} events (1 per ~7 ms, {KEYS} keys), tumbling W={W} ms, delay: 85% U[0,40) 12% U[40,400) 3% U[1000,3000) ms"
    );
    let _ = writeln!(
        o,
        "observed max delay = {maxd} ms; events whose arrival window != event window: {}",
        crossing_events(&events, W)
    );
    let _ = writeln!(
        o,
        "processing-time windows wrong: {} of {} windows (no late events, latency 0)",
        wrong_windows(&truth, &proc),
        truth
            .keys()
            .chain(proc.keys())
            .collect::<BTreeSet<_>>()
            .len()
    );

    let _ = writeln!(
        o,
        "\n-- per-window counts (excerpt: windows starting at 2000..6000 ms) --"
    );
    let excerpt = [
        (0u64, LatePolicy::Drop),
        (100, LatePolicy::Drop),
        (maxd, LatePolicy::Drop),
    ];
    let runs: Vec<Run> = excerpt
        .iter()
        .map(|&(d, p)| run_event_time(&events, &RunConfig::simple(W, d, p)))
        .collect();
    let _ = writeln!(
        o,
        "{:>8} {:>6} {:>6} {:>8} {:>10} {:>10}",
        "start", "truth", "proc", "ev D=0", "ev D=100", "ev D=max"
    );
    for s in (2000..=6000).step_by(W as usize) {
        let _ = writeln!(
            o,
            "{:>8} {:>6} {:>6} {:>8} {:>10} {:>10}",
            s,
            truth.get(&s).unwrap_or(&0),
            proc.get(&s).unwrap_or(&0),
            runs[0].final_counts.get(&s).unwrap_or(&0),
            runs[1].final_counts.get(&s).unwrap_or(&0),
            runs[2].final_counts.get(&s).unwrap_or(&0),
        );
    }

    let _ = writeln!(
        o,
        "\n-- trade-off: watermark delay D and allowed lateness L (ms; latency over windows ending <= {LATENCY_END_LIMIT}) --"
    );
    let _ = writeln!(
        o,
        "{:>5} {:>6} | {:>5} {:>5} {:>5} {:>6} | {:>8} {:>7} {:>8} {:>7} {:>5}",
        "D",
        "L",
        "late",
        "drop",
        "wrong",
        "refire",
        "1st mean",
        "1st max",
        "fin mean",
        "fin max",
        "flush"
    );
    let rows: [(u64, LatePolicy); 9] = [
        (0, LatePolicy::Drop),
        (50, LatePolicy::Drop),
        (200, LatePolicy::Drop),
        (500, LatePolicy::Drop),
        (1000, LatePolicy::Drop),
        (maxd, LatePolicy::Drop),
        (0, LatePolicy::AllowedLateness(500)),
        (0, LatePolicy::AllowedLateness(maxd)),
        (200, LatePolicy::AllowedLateness(maxd)),
    ];
    for (d, pol) in rows {
        let r = run_event_time(&events, &RunConfig::simple(W, d, pol));
        let l = r.latency(LATENCY_END_LIMIT);
        let lstr = match pol {
            LatePolicy::Drop => "-".to_string(),
            LatePolicy::AllowedLateness(x) => x.to_string(),
        };
        let _ = writeln!(
            o,
            "{:>5} {:>6} | {:>5} {:>5} {:>5} {:>6} | {:>8} {:>7} {:>8} {:>7} {:>5}",
            d,
            lstr,
            r.late,
            r.dropped,
            wrong_windows(&truth, &r.last_firing_counts()),
            r.refires(),
            l.first_mean,
            l.first_max,
            l.final_mean,
            l.final_max,
            r.flushed
        );
    }

    let _ = writeln!(
        o,
        "\n-- idle source: 2 partitions, partition 1 silent for ts in [4000,9000) then drained --"
    );
    let base = generate(seed, E, &DelayModel::SMALL);
    let outage = with_outage(base.clone(), 1, 4000, 9000);
    let t_truth = truth_counts(&base, W);
    for (name, idle) in [("no idle timeout", None), ("idle timeout 500ms", Some(500))] {
        let cfg = RunConfig {
            window: W,
            out_of_orderness: 50,
            policy: LatePolicy::Drop,
            partitions: 2,
            idle_timeout: idle,
        };
        let r = run_event_time(&outage, &cfg);
        let _ = writeln!(
            o,
            "{:<18} windows fired before t=9000: {:>2} | wm lag at t=8999: {:>5} ms | drops {:>3} | wrong {}/{}",
            name,
            r.first_fired_before(9000),
            wm_lag_at(&outage, &r, 8999),
            r.dropped,
            wrong_windows(&t_truth, &r.final_counts),
            t_truth.len()
        );
    }

    let _ = writeln!(
        o,
        "\n-- hopping windows (size 5000, slide 1000): one event, many windows --"
    );
    let _ = writeln!(
        o,
        "ts=7300 -> window starts {:?}",
        hopping_starts(7300, 5000, 1000)
    );
    let total: usize = events
        .iter()
        .map(|e| hopping_starts(e.ts, 5000, 1000).len())
        .sum();
    let _ = writeln!(
        o,
        "{} events -> {} window memberships (size/slide = 5 each); tumbling: 1 each",
        events.len(),
        total
    );
    o
}

/// Event-time distance between the newest event-time seen by processing time `t` and the
/// operator watermark at that point: how far the watermark lags behind "now".
fn wm_lag_at(events: &[Event], run: &Run, t: u64) -> i64 {
    let mut max_ts = 0i64;
    let mut wm = NEG;
    for (e, w) in events.iter().zip(&run.wm_trace) {
        if e.arrival > t {
            break;
        }
        max_ts = max_ts.max(e.ts as i64);
        wm = *w;
    }
    max_ts - wm
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u64 = 1000;
    const E: usize = 2000;

    fn ev(id: u64, ts: u64, arrival: u64) -> Event {
        Event {
            id,
            key: 0,
            part: 0,
            ts,
            arrival,
        }
    }

    /// Hand-made counterexample: the event with ts=8 arrives after ts=12 pushed the watermark
    /// past the end of window [0,10).
    #[test]
    fn hand_made_late_event_is_dropped_or_refired() {
        let events = vec![ev(0, 1, 1), ev(1, 12, 12), ev(2, 8, 13)];
        let truth = truth_counts(&events, 10);
        assert_eq!(truth[&0], 2);
        let drop = run_event_time(&events, &RunConfig::simple(10, 0, LatePolicy::Drop));
        assert_eq!((drop.dropped, drop.late, drop.refires()), (1, 1, 0));
        assert_eq!(drop.final_counts[&0], 1, "naive drop loses the event");
        // D=4: wm = 12-4 = 8 < 10, the window is still open when ts=8 arrives.
        let slack = run_event_time(&events, &RunConfig::simple(10, 4, LatePolicy::Drop));
        assert_eq!((slack.dropped, slack.final_counts[&0]), (0, 2));
        // Allowed lateness 5: wm=12 < 10+5, the late event re-fires the window with count 2.
        let lat = run_event_time(
            &events,
            &RunConfig::simple(10, 0, LatePolicy::AllowedLateness(5)),
        );
        assert_eq!((lat.dropped, lat.late, lat.refires()), (0, 1, 1));
        let first = lat.firings[0];
        assert_eq!(
            (first.window_start, first.count, first.fire_at, first.refire),
            (0, 1, 12, false)
        );
        let second = lat.firings.iter().find(|f| f.refire).unwrap();
        assert_eq!(
            (second.window_start, second.count, second.fire_at),
            (0, 2, 13)
        );
        // Allowed lateness 2: wm=12 >= 10+2, the window state is gone, so the event is dropped.
        let tight = run_event_time(
            &events,
            &RunConfig::simple(10, 0, LatePolicy::AllowedLateness(2)),
        );
        assert_eq!((tight.dropped, tight.refires()), (1, 0));
    }

    #[test]
    fn event_time_with_d_at_least_max_delay_is_exact_for_all_seeds() {
        for model in [DelayModel::HEAVY_TAIL, DelayModel::SMALL, DelayModel::NONE] {
            for seed in 0..120 {
                let events = generate(seed, E, &model);
                let truth = truth_counts(&events, W);
                for d in [observed_max_delay(&events), model.max_delay()] {
                    let r = run_event_time(&events, &RunConfig::simple(W, d, LatePolicy::Drop));
                    assert_eq!(r.dropped, 0, "seed {seed} d {d}");
                    assert_eq!(r.late, 0, "seed {seed} d {d}");
                    assert_eq!(r.final_counts, truth, "seed {seed} d {d}");
                    assert_eq!(wrong_windows(&truth, &r.last_firing_counts()), 0);
                }
            }
        }
    }

    /// Counterexample: a watermark delay of half the max delay loses events for every seed, and
    /// the loss is exactly the set of events that arrive behind the watermark.
    #[test]
    fn d_below_max_delay_loses_events() {
        for seed in 0..60 {
            let events = generate(seed, E, &DelayModel::HEAVY_TAIL);
            let d = observed_max_delay(&events) / 2;
            let r = run_event_time(&events, &RunConfig::simple(W, d, LatePolicy::Drop));
            assert!(r.dropped > 0, "seed {seed}");
            assert_eq!(r.dropped, r.late);
            let truth = truth_counts(&events, W);
            assert!(wrong_windows(&truth, &r.final_counts) > 0, "seed {seed}");
            assert_eq!(r.final_counts.values().sum::<u64>() + r.dropped, E as u64);
        }
    }

    #[test]
    fn processing_time_windows_are_wrong_whenever_an_event_crosses_a_boundary() {
        let mut crossed_seeds = 0;
        for seed in 0..120 {
            let events = generate(seed, E, &DelayModel::HEAVY_TAIL);
            let truth = truth_counts(&events, W);
            let proc = processing_time_counts(&events, W);
            let crossed = crossing_events(&events, W);
            // Arrivals only move events forward, so the earliest window that loses an event
            // cannot be refilled by anyone: it must differ.
            if crossed > 0 {
                crossed_seeds += 1;
                assert!(wrong_windows(&truth, &proc) > 0, "seed {seed}");
            } else {
                assert_eq!(proc, truth);
            }
            assert_eq!(
                proc.values().sum::<u64>(),
                E as u64,
                "no event lost, only moved"
            );
        }
        assert_eq!(
            crossed_seeds, 120,
            "heavy-tail delays cross a boundary for every seed"
        );
        // Control: with zero delay the processing-time answer equals the truth.
        let events = generate(1, E, &DelayModel::NONE);
        assert_eq!(processing_time_counts(&events, W), truth_counts(&events, W));
    }

    #[test]
    fn drops_do_not_increase_and_latency_does_not_decrease_with_d() {
        let ds = [0u64, 10, 25, 50, 100, 200, 400, 700, 1000, 2000, 3000];
        for seed in 0..60 {
            let events = generate(seed, E, &DelayModel::HEAVY_TAIL);
            let mut prev: Option<(u64, u64, u64)> = None;
            for d in ds {
                let r = run_event_time(&events, &RunConfig::simple(W, d, LatePolicy::Drop));
                let l = r.latency(LATENCY_END_LIMIT);
                let cur = (r.dropped, l.first_mean, l.first_max);
                if let Some(p) = prev {
                    assert!(cur.0 <= p.0, "seed {seed} d {d}: drops {} > {}", cur.0, p.0);
                    assert!(cur.1 >= p.1, "seed {seed} d {d}: mean latency");
                    assert!(cur.2 >= p.2, "seed {seed} d {d}: max latency");
                }
                prev = Some(cur);
            }
            // Strict at both ends: D=0 drops, D=max does not, and the latency actually grows.
            let first = run_event_time(&events, &RunConfig::simple(W, 0, LatePolicy::Drop));
            let last = run_event_time(&events, &RunConfig::simple(W, 3000, LatePolicy::Drop));
            assert!(first.dropped > 0 && last.dropped == 0);
            assert!(
                last.latency(LATENCY_END_LIMIT).first_mean
                    > first.latency(LATENCY_END_LIMIT).first_mean
            );
        }
    }

    #[test]
    fn without_allowed_lateness_every_window_fires_exactly_once() {
        for seed in 0..80 {
            let events = generate(seed, E, &DelayModel::HEAVY_TAIL);
            for d in [0, 100, 1000, 3000] {
                let r = run_event_time(&events, &RunConfig::simple(W, d, LatePolicy::Drop));
                assert_eq!(r.refires(), 0);
                let mut seen = BTreeSet::new();
                for f in &r.firings {
                    assert!(
                        seen.insert(f.window_start),
                        "seed {seed} d {d}: window fired twice"
                    );
                    // The fired count is the final count.
                    assert_eq!(r.final_counts[&f.window_start], f.count);
                }
                assert_eq!(seen, r.final_counts.keys().copied().collect());
                // Every event is either counted or dropped.
                assert_eq!(r.final_counts.values().sum::<u64>() + r.dropped, E as u64);
            }
        }
    }

    #[test]
    fn allowed_lateness_converges_to_truth_at_the_price_of_refires_and_final_latency() {
        for seed in 0..80 {
            let events = generate(seed, E, &DelayModel::HEAVY_TAIL);
            let truth = truth_counts(&events, W);
            let maxd = observed_max_delay(&events);
            let drop = run_event_time(&events, &RunConfig::simple(W, 0, LatePolicy::Drop));
            let lat = run_event_time(
                &events,
                &RunConfig::simple(W, 0, LatePolicy::AllowedLateness(maxd)),
            );
            // FINAL (= last re-fired) counts are the ground truth; the first results were not.
            assert_eq!(lat.dropped, 0);
            assert_eq!(lat.last_firing_counts(), truth, "seed {seed}");
            assert_eq!(lat.final_counts, truth);
            assert!(lat.refires() > 0 && lat.refires() as u64 == lat.late);
            let first: BTreeMap<u64, u64> = lat
                .firings
                .iter()
                .filter(|f| !f.refire)
                .map(|f| (f.window_start, f.count))
                .collect();
            assert!(
                wrong_windows(&truth, &first) > 0,
                "first results are not final"
            );
            // Same first-fire latency as Drop at the same D; the answer is final only later.
            assert_eq!(
                lat.latency(LATENCY_END_LIMIT).first_mean,
                drop.latency(LATENCY_END_LIMIT).first_mean
            );
            assert!(
                lat.latency(LATENCY_END_LIMIT).final_mean
                    > drop.latency(LATENCY_END_LIMIT).final_mean
            );
            // Too little lateness (L=100) still loses some events; drops shrink as L grows.
            let small = run_event_time(
                &events,
                &RunConfig::simple(W, 0, LatePolicy::AllowedLateness(100)),
            );
            assert!(small.dropped > 0 && small.dropped < drop.dropped);
            assert!(wrong_windows(&truth, &small.last_firing_counts()) > 0);
        }
    }

    #[test]
    fn watermark_is_monotonic_and_never_exceeds_max_ts_minus_d() {
        for seed in 0..80 {
            let events = generate(seed, E, &DelayModel::HEAVY_TAIL);
            for d in [0, 300, 3000] {
                let r = run_event_time(&events, &RunConfig::simple(W, d, LatePolicy::Drop));
                assert!(r.wm_trace.windows(2).all(|p| p[0] <= p[1]), "seed {seed}");
                let mut max_ts = 0i64;
                for (e, wm) in events.iter().zip(&r.wm_trace) {
                    max_ts = max_ts.max(e.ts as i64);
                    assert!(*wm <= max_ts - d as i64);
                    // Single partition, no idleness: the watermark is exactly the heuristic.
                    assert_eq!(*wm, max_ts - d as i64);
                }
            }
            // Also with an outage and an idle timeout (the min over partitions must not go back).
            let outage = with_outage(generate(seed, E, &DelayModel::SMALL), 1, 4000, 9000);
            for idle in [None, Some(500), Some(100)] {
                let cfg = RunConfig {
                    window: W,
                    out_of_orderness: 50,
                    policy: LatePolicy::Drop,
                    partitions: 2,
                    idle_timeout: idle,
                };
                let r = run_event_time(&outage, &cfg);
                assert!(
                    r.wm_trace.windows(2).all(|p| p[0] <= p[1]),
                    "seed {seed} idle {idle:?}"
                );
            }
        }
    }

    #[test]
    fn idle_partition_stalls_the_min_watermark_and_idle_timeout_unstalls_it() {
        let (from, to) = (4000, 9000);
        for seed in 0..60 {
            let base = generate(seed, E, &DelayModel::SMALL);
            let truth = truth_counts(&base, W);
            let outage = with_outage(base.clone(), 1, from, to);
            let mk = |idle| RunConfig {
                window: W,
                out_of_orderness: 50,
                policy: LatePolicy::Drop,
                partitions: 2,
                idle_timeout: idle,
            };
            let stalled = run_event_time(&outage, &mk(None));
            let unstalled = run_event_time(&outage, &mk(Some(500)));
            // Control: partition 1 healthy => the heuristic with D=50 >= max small delay is exact.
            let healthy = run_event_time(&base, &mk(None));
            assert_eq!((healthy.dropped, healthy.final_counts == truth), (0, true));

            // Stalled: during the outage only windows ending before the silent point may fire.
            assert!(
                stalled.first_fired_before(to) as u64 <= from / W,
                "seed {seed}"
            );
            assert!(
                wm_lag_at(&outage, &stalled, to - 1) > 4000,
                "wm lags far behind now"
            );
            // ... but nothing is lost: the drained backlog is still inside the open windows.
            assert_eq!(
                (stalled.dropped, stalled.final_counts == truth),
                (0, true),
                "seed {seed}"
            );

            // Unstalled: windows keep firing during the outage ...
            assert!(
                unstalled.first_fired_before(to) > stalled.first_fired_before(to) + 3,
                "seed {seed}"
            );
            assert!(wm_lag_at(&outage, &unstalled, to - 1) < 300);
            // ... at the price of treating the drained backlog as late data (counterexample).
            assert!(unstalled.dropped > 0, "seed {seed}");
            assert!(wrong_windows(&truth, &unstalled.final_counts) > 0);
        }
    }

    #[test]
    fn tumbling_windows_are_disjoint_and_cover_time() {
        let w = 1000;
        let mut prev_start = 0;
        for ts in 0..10_000u64 {
            let s = tumbling_start(ts, w);
            assert!(s <= ts && ts < s + w && s.is_multiple_of(w));
            assert!(s >= prev_start && s - prev_start <= w);
            if s != prev_start {
                // The next window starts exactly where the previous one ends: no gap, no overlap.
                assert_eq!(s, prev_start + w);
            }
            prev_start = s;
        }
        // Every event belongs to exactly one window: the counts add up.
        let events = generate(5, E, &DelayModel::HEAVY_TAIL);
        assert_eq!(truth_counts(&events, w).values().sum::<u64>(), E as u64);
    }

    #[test]
    fn hopping_assigns_size_over_slide_windows_and_tumbling_is_the_special_case() {
        for (size, slide) in [(5000u64, 1000u64), (1000, 1000), (6, 2), (12, 3), (10, 1)] {
            for ts in size..size * 5 {
                let starts = hopping_starts(ts, size, slide);
                assert_eq!(
                    starts.len() as u64,
                    size / slide,
                    "ts {ts} size {size} slide {slide}"
                );
                assert!(starts.iter().all(|&s| s % slide as i64 == 0
                    && s <= ts as i64
                    && (ts as i64) < s + size as i64));
                assert!(starts.windows(2).all(|p| p[1] - p[0] == slide as i64));
            }
        }
        // size == slide is the tumbling assignment.
        for ts in 0..5000 {
            assert_eq!(
                hopping_starts(ts, 1000, 1000),
                vec![tumbling_start(ts, 1000) as i64]
            );
        }
        // Pinned: ts=7300, size 5000, slide 1000 -> windows starting 3000..=7000.
        assert_eq!(
            hopping_starts(7300, 5000, 1000),
            vec![3000, 4000, 5000, 6000, 7000]
        );
        // Near the epoch the lab allows windows starting before 0.
        assert_eq!(hopping_starts(500, 3000, 1000), vec![-2000, -1000, 0]);
    }

    /// The delay model never changes event times: same seed => same ground truth.
    #[test]
    fn event_times_do_not_depend_on_the_delay_model() {
        for seed in 0..20 {
            let by_id = |mut v: Vec<Event>| {
                v.sort_by_key(|e| e.id);
                v.into_iter()
                    .map(|e| (e.id, e.key, e.ts))
                    .collect::<Vec<_>>()
            };
            let a = by_id(generate(seed, E, &DelayModel::HEAVY_TAIL));
            let b = by_id(generate(seed, E, &DelayModel::SMALL));
            assert_eq!(a, b);
        }
        let heavy = generate(42, E, &DelayModel::HEAVY_TAIL);
        let small = generate(42, E, &DelayModel::SMALL);
        assert_eq!(truth_counts(&heavy, W), truth_counts(&small, W));
        assert_ne!(
            processing_time_counts(&heavy, W),
            processing_time_counts(&small, W)
        );
    }

    #[test]
    fn generation_is_deterministic_and_arrival_sorted() {
        let a = generate(42, E, &DelayModel::HEAVY_TAIL);
        assert_eq!(a, generate(42, E, &DelayModel::HEAVY_TAIL));
        assert_ne!(a, generate(43, E, &DelayModel::HEAVY_TAIL));
        assert!(a.windows(2).all(|p| p[0].arrival <= p[1].arrival));
        assert!(a.iter().all(|e| e.arrival >= e.ts));
        assert!(observed_max_delay(&a) <= DelayModel::HEAVY_TAIL.max_delay());
        assert_eq!(report(42), report(42));
    }
}
