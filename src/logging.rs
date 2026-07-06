use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Once, OnceLock},
};

use anyhow::Context;
use parking_lot::Mutex;
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

static LOGGING_INIT: Once = Once::new();
static LOGGING_INIT_RESULT: OnceLock<Result<(), String>> = OnceLock::new();

pub fn init_logging(log_file: Option<&Path>) -> anyhow::Result<()> {
    LOGGING_INIT.call_once(|| {
        let result = (|| -> anyhow::Result<()> {
            let writer = LogSink::from_path(log_file)?;
            let env_filter =
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
            let subscriber = tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(writer)
                .with_ansi(false)
                .without_time()
                .finish();

            tracing::subscriber::set_global_default(subscriber)
                .context("failed to install global tracing subscriber")?;
            Ok(())
        })()
        .map_err(|error| format!("{error:#}"));

        let _ = LOGGING_INIT_RESULT.set(result);
    });

    match LOGGING_INIT_RESULT.get() {
        Some(Ok(())) => Ok(()),
        Some(Err(message)) => Err(anyhow::anyhow!(message.clone())),
        None => Ok(()),
    }
}

pub fn install_panic_hook(log_file: Option<PathBuf>) {
    std::panic::set_hook(Box::new(move |panic_info| {
        let message = format!("lymals panic: {panic_info}\n");
        if let Some(path) = &log_file
            && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
        {
            let _ = file.write_all(message.as_bytes());
            return;
        }
        let _ = io::stderr().write_all(message.as_bytes());
    }));
}

#[derive(Clone)]
enum LogSink {
    Stderr,
    File(Arc<Mutex<File>>),
}

impl LogSink {
    fn from_path(path: Option<&Path>) -> anyhow::Result<Self> {
        match path {
            Some(path) => {
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .with_context(|| format!("failed to open log file {}", path.display()))?;
                Ok(Self::File(Arc::new(Mutex::new(file))))
            }
            None => Ok(Self::Stderr),
        }
    }
}

impl<'a> MakeWriter<'a> for LogSink {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        match self {
            Self::Stderr => LogWriter::Stderr(io::stderr()),
            Self::File(file) => LogWriter::File(file.clone()),
        }
    }
}

enum LogWriter {
    Stderr(io::Stderr),
    File(Arc<Mutex<File>>),
}

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stderr(stderr) => stderr.write(buf),
            Self::File(file) => file.lock().write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stderr(stderr) => stderr.flush(),
            Self::File(file) => file.lock().flush(),
        }
    }
}
