use local_common::term::{color_enabled_for, Colour};
use std::env;
use std::io;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut stdout = io::stdout();

    match webdriver::run(&args, &mut stdout) {
        Ok(code) => process::exit(code),
        Err(err) => {
            let c = Colour::new(color_enabled_for(&io::stderr(), false));
            eprintln!("{} {err}", c.red("err"));
            process::exit(1);
        }
    }
}
