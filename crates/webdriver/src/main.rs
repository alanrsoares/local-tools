use std::env;
use std::io;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut stdout = io::stdout();

    match webdriver::run(&args, &mut stdout) {
        Ok(code) => process::exit(code),
        Err(err) => {
            eprintln!("\x1b[1;31merror:\x1b[0m {err}");
            process::exit(1);
        }
    }
}
