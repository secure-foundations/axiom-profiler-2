use std::path::PathBuf;

use smt_log_parser::{
    analysis::{InstGraph, QuantifierAnalysis},
    items::QuantIdx,
    Z3Parser,
};

pub fn run(logfile: PathBuf, _depth: Option<u32>, _pretty_print: bool) -> Result<(), String> {
    let parser = super::run_on_logfile(logfile)?;
    let inst_graph = InstGraph::new(&parser).map_err(|e| format!("{e:?}"))?;

    let qanalysis = QuantifierAnalysis::new(&parser, &inst_graph);
    // let total_costs = qanalysis.total_costs();
    fn get_quant_name(parser: &Z3Parser, qidx: QuantIdx) -> &str {
        parser[qidx].kind.user_name().map(|name| &parser[name]).unwrap_or("unnamed")
    }

    for (q_pat, info) in qanalysis.iter_enumerated() {
        let qidx = q_pat.quant;
        let name = get_quant_name(&parser, qidx);

        let cur_cost = info.costs;
        let sub_total = info.values().sum::<u32>();
        println!("{name} {:?} {cur_cost}", qidx);

        for (dep_qidx, count) in info.iter() {
            let dep_name = get_quant_name(&parser, dep_qidx);
            println!("\t{dep_name} {:?} {count} {sub_total}", dep_qidx);
        }

        // let named_count = named().count();
        // if info.direct_deps.len() == named_count {
        //     println!("axiom {name} ({percentage:.1}%) depends on {named_count} axioms:");
        // } else {
        //     println!(
        //         "axiom {name} ({percentage:.1}%) depends on {} axioms, of those {named_count} are named:",
        //         info.direct_deps.len(),
        //     );
        // }
        // for (dep, count) in named() {
        //     // let percentage = 100.0 * count as f64 / total;
        //     println!("\t{dep} ({percentage:.1}%)");
        // }
        //  else {
        //     let deps: Vec<String> = named()
        //         .map(|(dep, count)| {
        //             let percentage = 100.0 * count as f64 / total;
        //             format!("{dep} ({percentage:.1}%)")
        //         })
        //         .collect();
        //     let named_count = deps.len();
        //     if info.direct_deps.len() == named_count {
        //         println!("{name} ({percentage:.1}%) -> {}", deps.join(", "));
        //     } else {
        //         println!(
        //             "{name} ({percentage:.1}%), {named_count}/{} named -> {}",
        //             info.direct_deps.len(),
        //             deps.join(", ")
        //         );
        //     }
        }


    // let trans = qanalysis.calculate_transitive(depth);

    // for (qidx, deps) in trans.iter_enumerated() {
    //     let Some(name) = get_quant_name(&parser, qidx) else {
    //         continue;
    //     };
    //     let costs = qanalysis.quant_sum_cost(qidx);
    //     let percentage = (100.0 * costs as f64) / total_costs as f64;
    //     let named = || deps.iter().flat_map(|ddep| get_quant_name(&parser, *ddep));
    //     if pretty_print {
    //         println!(
    //             "axiom {name} ({percentage:.1}%) depends on {} axioms:",
    //             deps.len()
    //         );
    //         for dep in named() {
    //             println!(" - {dep}");
    //         }
    //     } else {
    //         let deps: Vec<_> = named().collect();
    //         println!("{name} ({percentage:.1}%) = {}", deps.join(", "));
    //     }
    // }

    Ok(())
}
