use serde::Deserialize;
use smt_log_parser::display_with::DisplayWithCtxt;
use smt_log_parser::items::Quantifier;
use smt_log_parser::parsers::z3::{z3parser::Z3Parser, Z3LogParser};
use smt_log_parser::parsers::LogParser;
use smt_log_parser::items::{MatchKind, QuantKind, TermIdx};
use smt_log_parser::display_with::{DisplayCtxt};
use std::collections::HashMap;
use std::{borrow::Cow, env, time::Duration};
use wasm_timer::Instant;

fn main() {
    let args: Vec<String> = env::args().collect();
    let filenames = &args[1..];

    for path in &*filenames {
        let path = std::path::Path::new(path);
        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy())
            .unwrap_or_default();
        if !path.is_file() {
            println!("Skipping {filename:?}");
            continue;
        }
        println!("Parsing {filename:?}");
        let time = Instant::now();
        // // Use to test max loading speed
        // let file = std::fs::read_to_string(path).unwrap();
        // let len = file.chars().filter(|c| *c == '\n').count();
        // let parsed = StreamParser::parse_entire_string(&file, Duration::from_secs_f32(10.0));
        // let to = Duration::from_secs_f32(15.0);
        let (_metadata, parser) = Z3Parser::from_file(path).unwrap();
        let parser = parser.process_all().unwrap();

        let mut qid_to_usage_count: HashMap<&str, _> = HashMap::new();

        let ctxt = DisplayCtxt {
            parser: &parser,
            display_term_ids: false,
            display_quantifier_name: false,
            use_mathematical_symbols: true,
        };

        for inst in &parser.insts.matches {
            match &inst.kind {
                MatchKind::Quantifier { quant, pattern, bound_terms } => {
                    if let Quantifier { kind: QuantKind::NamedQuant(name_id), .. } = parser.quantifiers[*quant] {
                        let name = &parser.strings[name_id];

                        print!("qi {name}");
                        for bound_term in bound_terms {
                            print!(" {}", TermIdx(bound_term.0).with(&ctxt));
                        }
                        println!();

                        if !qid_to_usage_count.contains_key(name) {
                            qid_to_usage_count.insert(name, 0);
                        }
                        qid_to_usage_count.insert(name, qid_to_usage_count[name] + 1);
                    }
                },

                _ => {},
            }
        }

        // let mut qid_count_pairs = qid_to_usage_count.iter().collect::<Vec<_>>();
        // qid_count_pairs.sort_by_key(|(_, count)| **count);

        // for (qid, count) in qid_count_pairs {
        //     println!("qid {qid:?} instantiated {count:?} time(s)");
        // }

        let elapsed_time = time.elapsed();
        // println!(
        //     "{} parsing after {} seconds (timeout {timeout:?})\n",
        //     if timeout.is_timeout() { "Timeout" } else { "Finished" }, elapsed_time.as_secs_f32()
        // );
        println!("Finished parsing after {} seconds", elapsed_time.as_secs_f32())
        // result.save_output_to_files(&settings, &time);
        // let render_engine = GraphVizRender;
        // let _svgparser = render_engine.make_svg(OUT_DOT, OUT_SVG);
        // add_link_to_svg(OUT_SVG, OUT_SVG_2);
        // println!(
        //     "Finished render sequence after {} seconds",
        //     time.elapsed().as_secs_f32()
        // );

        // let elapsed_time = time.elapsed();
        // println!("Done, run took {} seconds.", elapsed_time.as_secs_f32());
    }
}
