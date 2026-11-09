//! Headless authoring/QA helper. Reads one JSON request from stdin and writes
//! one JSON report to stdout; no input providers or desktop output are linked.
use serde::Deserialize;
#[derive(Deserialize)]
struct Request {
    filter: motion_filters::FilterConfig,
    signal: motion_filters::PreviewSignal,
}
fn main() {
    let result = (|| -> anyhow::Result<_> {
        let input: Request = serde_json::from_reader(std::io::stdin())?;
        motion_filters::preview(input.filter, input.signal)
    })();
    match result {
        Ok(report) => println!("{}", serde_json::to_string(&report).unwrap()),
        Err(error) => {
            println!("{}", serde_json::json!({"error":format!("{error:#}")}));
            std::process::exit(1);
        }
    }
}
