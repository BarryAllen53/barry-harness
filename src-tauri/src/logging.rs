use std::{
    env,
    fs::{self, File, OpenOptions},
    io,
    path::PathBuf,
    sync::Mutex,
};

use tracing_subscriber::fmt::writer::BoxMakeWriter;

const LOG_FILE: &str = "barry.log";

pub fn init() {
    let (writer, failure) = match open_log_file() {
        Ok(file) => (BoxMakeWriter::new(Mutex::new(file)), None),
        Err(error) => (BoxMakeWriter::new(io::stderr), Some(error)),
    };
    tracing_subscriber::fmt().with_writer(writer).init();
    if let Some(error) = failure {
        tracing::warn!(%error, "the log file could not be opened, logging to stderr");
    }
}

fn log_directory() -> io::Result<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(|base| PathBuf::from(base).join("BarryHarness").join("logs"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))
}

fn open_log_file() -> io::Result<File> {
    let directory = log_directory()?;
    fs::create_dir_all(&directory)?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join(LOG_FILE))
}
