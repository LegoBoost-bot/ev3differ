use std::{
    env::current_dir,
    fs::File,
    io::{self},
    path::PathBuf,
};

use clap::{Parser, Subcommand};
use zip_extensions::zip_create_from_directory;
use zip_extract::extract;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// Extract an EV3 archive to a directory
    Extract { src: PathBuf, dst: Option<PathBuf> },

    /// Put the contents of a directory in an EV3 file
    Archive { src: PathBuf, dst: Option<PathBuf> },
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Command::Extract { src, dst } => {
            let dir = dst.unwrap_or_else(|| {
                let mut pwd = current_dir().unwrap();
                pwd.push(src.file_stem().unwrap());
                pwd
            });

            extract(File::open(src)?, &dir, true).unwrap();

            eprintln!("Successfully extracted into {}", dir.display());
        }
        Command::Archive { src, dst } => {
            let filename = match dst {
                Some(dst) => dst,
                None => PathBuf::from(src.with_extension("ev3")),
            };
            zip_create_from_directory(&filename, &src)?;

            eprintln!("Successfully archived into {}", filename.display());
        }
    }

    Ok(())
}
