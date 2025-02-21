use std::{
    env::current_dir,
    fs::{self, File},
    io::{self, Read, Seek, Write},
    path::Path,
    process::Command,
};

use clap::Parser;
use walkdir::{DirEntry, WalkDir};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    git: Vec<String>,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    let pwd = current_dir()?;
    let file = pwd.with_extension("ev3");

    extract_file(&file, current_dir().unwrap())?;
    Command::new("git").args(cli.git).spawn()?.wait()?;

    archive_file(&pwd, &file)?;

    Ok(())
}

fn zip_dir(
    it: &mut dyn Iterator<Item = DirEntry>,
    prefix: impl AsRef<Path>,
    writer: impl Write + Seek,
) -> io::Result<()> {
    let mut zip = ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut buffer = Vec::new();
    for entry in it {
        let path = entry.path();
        let name = path.strip_prefix(&prefix).unwrap();

        let path_as_string = name.to_str().expect("non-UTF8 path");

        if path.is_file() {
            zip.start_file(path_as_string, options)?;

            let mut f = File::open(path)?;

            f.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
            buffer.clear();
        } else if !name.as_os_str().is_empty() {
            zip.add_directory(path_as_string, options)?;
        }
    }

    zip.finish()?;
    Ok(())
}

fn archive_file(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    if !src.as_ref().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "cannot archive single files",
        ));
    }

    let file = File::create(dst)?;

    let it = WalkDir::new(src.as_ref()).into_iter();

    zip_dir(&mut it.flatten(), src, file)
}

fn extract_file(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    let file = File::open(src)?;

    let mut archive = ZipArchive::new(file)?;

    if !dst.as_ref().is_dir() {
        fs::create_dir(&dst)?;
    }

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let filepath = match file.enclosed_name() {
            Some(path) => path,
            None => continue,
        };

        if filepath.starts_with(".git") {
            continue;
        }

        let mut outpath = dst.as_ref().to_owned();
        outpath.push(filepath);

        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p)?;
                }
            }

            let mut outfile = match File::create(&outpath) {
                Ok(f) => f,
                Err(_) => {
                    eprintln!("Falied to extract '{}'", outpath.display());
                    continue;
                }
            };
            io::copy(&mut file, &mut outfile)?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Some(mode) = file.unix_mode() {
                fs::set_permissions(&outpath, fs::Permissions::from_mode(mode)).unwrap();
            }
        }
    }

    Ok(())
}

