use std::{
    env::current_dir,
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read, Seek, Write},
    path::Path,
    process::Command,
};

use chrono::{DateTime, Local};
use clap::Parser;
use git2::Repository;
use regex::{Captures, Regex};
use walkdir::{DirEntry, WalkDir};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    git: Vec<String>,

    #[arg(short, long)]
    init: Option<String>,

    #[arg(short, long)]
    make_version: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let pwd = current_dir()?;
    let file = pwd.with_extension("ev3");

    if let Some(url) = cli.init {
        Repository::clone(&url, &pwd)?;
    } else {
        cleanup(&pwd)?;
        extract_file(&file, current_dir().unwrap())?;
        if cli.make_version {
            make_version(&pwd)?;
        } else {
            Command::new("git").args(cli.git).spawn()?.wait()?;
        }
        update_commit_id(&pwd)?;
    }

    archive_file(&pwd, &file)?;
    post(&pwd)?;

    Ok(())
}

fn make_version(src: impl AsRef<Path>) -> io::Result<()> {
    let regex =
        Regex::new(r#"<ConfigurableMethodTerminal\s+ConfiguredValue="\{EV3DIFFER\s+(.*)\}">"#)
            .unwrap();

    let mut buf = String::new();

    for f in WalkDir::new(&src)
        .into_iter()
        .flatten()
        .map(|e| e.into_path())
        .filter(|e| e.is_file())
        .filter(|e| e.extension() == Some(&OsStr::new("ev3p")))
    {
        buf.clear();
        File::open(&f).unwrap().read_to_string(&mut buf).unwrap();

        let mut is_modified = false;

        let new = regex.replace_all(&buf, |c: &Captures| {
            is_modified = true;
            format!("{} <!-- EV3DIFFER track {} -->", &c[0], &c[1])
        });

        if is_modified {
            eprintln!("Modifying file {}", f.display());
            File::create(&f).unwrap().write(new.as_bytes()).unwrap();
        }
    }

    Ok(())
}

fn update_commit_id(src: impl AsRef<Path>) -> Result<(), git2::Error> {
    let repo = Repository::init(&src)?;

    let head = repo.head()?;
    let commit = head.peel_to_commit()?;
    let time = DateTime::from_timestamp(commit.time().seconds(), 0)
        .unwrap()
        .with_timezone(&Local);

    let regex = Regex::new(
        r#"<ConfigurableMethodTerminal\s+ConfiguredValue="(.*)">\s*<!--\s*EV3DIFFER track ([a-z]+)\s*-->"#,
    )
    .unwrap();

    let mut buf = String::new();

    for f in WalkDir::new(&src)
        .into_iter()
        .flatten()
        .map(|e| e.into_path())
        .filter(|e| e.is_file())
        .filter(|e| e.extension() == Some(&OsStr::new("ev3p")))
    {
        buf.clear();
        File::open(&f).unwrap().read_to_string(&mut buf).unwrap();

        let mut is_modified = false;

        let new = regex
            .replace_all(&buf, |c: &Captures| {
                is_modified = true;
                let track = &c[2];
                let inside = match track {
                    "time" => format!("{}", time.format("%a %d/%m/%Y %H:%M")),
                    "name" => head.shorthand().unwrap().into(),
                    "msg" => commit.message().unwrap().into(),
                    "hash" => commit.id().to_string()[0..7].to_string(),
                    o => panic!("Unknown tracking type `{o}`"),
                };

                format!(r#"<ConfigurableMethodTerminal ConfiguredValue="{inside}"> <!-- EV3DIFFER track {track} -->"#)
            });

        if is_modified {
            File::create(&f).unwrap().write(new.as_bytes()).unwrap();
        }
    }

    Ok(())
}

fn post(src: impl AsRef<Path>) -> io::Result<()> {
    for f in WalkDir::new(src)
        .into_iter()
        .flatten()
        .filter(|e| e.depth() == 1)
    {
        if f.path().is_dir() && f.file_name() == ".git" {
            fs::rename(f.path(), f.path().with_file_name(".ev3git"))?;
        }
    }

    Ok(())
}

fn cleanup(src: impl AsRef<Path>) -> io::Result<()> {
    for f in WalkDir::new(src)
        .into_iter()
        .flatten()
        .filter(|e| e.depth() == 1)
    {
        if f.path().is_dir() {
            if f.file_name() == ".ev3git" {
                fs::rename(f.path(), f.path().with_file_name(".git"))?;
            }
        } else {
            fs::remove_file(f.path())?;
        }
    }

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
