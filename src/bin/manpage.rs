use clap::CommandFactory;
use std::fs;
use std::io;
use std::path::Path;

fn main() -> io::Result<()> {
    let cmd = bop::cli::Cli::command();
    let man_dir = Path::new("man");
    fs::create_dir_all(man_dir)?;

    clap_mangen::generate_to(cmd, man_dir)?;

    // List generated files
    for entry in fs::read_dir(man_dir)? {
        let entry = entry?;
        println!("Generated {}", entry.path().display());
    }

    Ok(())
}
