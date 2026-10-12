//! Lab (a): replication lag, read policies, ack levels, failover loss, consistent prefix.
//! Usage: `replication_lag [seed=42] [runs=10]` — results are summed over seeds `seed..seed+runs`.

use lab_distributed::replication::{
    Ack, Config, Policy, Report, consistent_prefix_demo, pct1, ratio1, run_many,
};

const LAGS: [u64; 3] = [0, 20, 80];
/// Lag used for the ack-level table.
const ACK_LAG: u64 = 40;

fn main() {
    let mut args = std::env::args().skip(1);
    let seed: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(42);
    let runs: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(10);
    let base = Config::new(10, Ack::Async, Policy::Leader);
    println!(
        "Lab (a) replication_lag: seeds {seed}..{} ({runs} runs, summed)",
        seed + runs - 1
    );
    println!(
        "F={} followers (last = slow), {} sessions x {} rounds, {} keys, think={} ms, {} reads/round {} ms apart",
        base.followers,
        base.sessions,
        base.rounds,
        base.keys,
        base.think,
        base.reads_per_round,
        base.read_gap
    );
    println!(
        "lag = total lag of the slow follower: recv lag/4 then replay 3*lag/4; fast follower = 1/4 of that; jitter <= stage/4\n"
    );

    println!("[1] read policy, ack=Async: RYW% / MR% / leader-read%");
    println!(
        "{}",
        format!(
            "{:<22}{:^21}{:^23}{:^23}",
            "policy", "lag=0 ms", "lag=20 ms", "lag=80 ms"
        )
        .trim_end()
    );
    println!(
        "{:<22}{}",
        "",
        "   RYW%    MR%  lead%  ".repeat(3).trim_end()
    );
    let policies = [
        Policy::Leader,
        Policy::RandomFollower,
        Policy::StickyFollower,
        Policy::LeaderForWindow(30),
        Policy::LeaderForWindow(120),
        Policy::Token,
    ];
    for p in policies {
        let mut line = format!("{:<22}", p.label());
        for lag in LAGS {
            let r = run_many(&Config::new(lag, Ack::Async, p), seed, runs);
            line += &format!(
                "{:>7}{:>7}{:>7}  ",
                pct1(r.ryw, r.reads),
                pct1(r.mr, r.reads),
                pct1(r.leader_reads, r.reads)
            );
        }
        println!("{}", line.trim_end());
    }

    println!(
        "\n[2] ack level at lag={ACK_LAG} ms, reads=RandomFollower; failover = 200 random leader crashes per run"
    );
    println!(
        "{:<14}{:>9}{:>8}{:>8}{:>8}{:>13}{:>15}",
        "ack", "lat mean", "lat max", "RYW%", "MR%", "lost/crash", "crash w/loss%"
    );
    for ack in [
        Ack::Async,
        Ack::Received(1),
        Ack::Received(2),
        Ack::Applied(1),
        Ack::Applied(2),
    ] {
        let mut c = Config::new(ACK_LAG, ack, Policy::RandomFollower);
        c.crash_probes = 200;
        let r = run_many(&c, seed, runs);
        println!(
            "{:<14}{:>9}{:>8}{:>8}{:>8}{:>13}{:>15}",
            ack.label(c.followers),
            ratio1(r.lat_sum, r.writes),
            r.lat_max,
            pct1(r.ryw, r.reads),
            pct1(r.mr, r.reads),
            ratio1(r.lost_acked, r.crashes),
            pct1(r.crashes_with_loss, r.crashes)
        );
    }
    let loss = |lag: u64| -> Report {
        let mut c = Config::new(lag, Ack::Async, Policy::Leader);
        c.crash_probes = 200;
        run_many(&c, seed, runs)
    };
    let cells: Vec<String> = LAGS
        .iter()
        .map(|&l| format!("lag={l}: {}", ratio1(loss(l).lost_acked, loss(l).crashes)))
        .collect();
    println!(
        "Async acked writes lost per crash by lag: {}",
        cells.join("  ")
    );

    println!(
        "\n[3] consistent prefix: 40 order+payment pairs, 500 probes per seed; probes showing 'payment without its order'"
    );
    println!("{:<8}{:>15}{:>12}", "lag", "2 logs (shards)", "1 log");
    for lag in LAGS {
        let (mut probes, mut indep, mut single) = (0, 0, 0);
        for s in seed..seed + runs {
            let r = consistent_prefix_demo(s, 40, lag);
            probes += r.probes;
            indep += r.independent;
            single += r.single_log;
        }
        println!(
            "{lag:<8}{:>15}{:>12}",
            format!("{indep}/{probes}"),
            format!("{single}/{probes}")
        );
    }
}
