use clap::Parser;
use lottr::{start, Arguments};

#[tokio::main]
async fn main() {
    // let args = Arguments {
    //     output_only: false,
    //     input: Some("./assets/haha.txt".to_string()),
    //     template: "./assets/options_01.toml".to_string(),
    // };
    let args = Arguments::parse();
    start(args).await.unwrap();
}
