//! Lab (e): "every request that is pending is eventually done", under three levels of fairness
//! (P13 chapter 05, section 6).
//!
//!   cargo run -q -p lab-tla --bin liveness
//!
//! The model mirrors `specs/RequestService.tla`. The checker looks for a fair cycle in which a
//! request stays pending forever, the way TLC searches for a lasso.

use lab_tla::request::{RequestService, Spec};

fn main() {
    let model = RequestService { reqs: 2 };
    let g = model.graph();
    println!(
        "state graph of RequestService with Reqs = {{r1, r2}}: {} distinct states, {} generated, depth {}",
        g.stats.distinct, g.stats.generated, g.stats.depth
    );
    for (spec, what) in [
        (Spec::Spec0, "Spec0: no fairness"),
        (Spec::Spec1, "Spec1: WF_vars(Restart), WF_vars(Process(r))"),
        (Spec::Spec2, "Spec2: WF_vars(Restart), SF_vars(Process(r))"),
    ] {
        println!("\n== {what} ==");
        for f in model.fairness(spec) {
            println!("fairness: {}", f.name);
        }
        for r in 0..2 {
            match model.all_served(&g, spec, r) {
                None => println!(
                    "(req[r{0}] = \"pending\") ~> (req[r{0}] = \"done\"): holds",
                    r + 1
                ),
                Some(lasso) => {
                    println!(
                        "(req[r{0}] = \"pending\") ~> (req[r{0}] = \"done\"): VIOLATED; lasso with a prefix of {1} states and a cycle of {2} steps",
                        r + 1,
                        lasso.prefix.len(),
                        lasso.cycle.len()
                    );
                    if r == 0 {
                        print!("{}", lasso.render(&RequestService::show));
                    }
                }
            }
        }
    }
}
