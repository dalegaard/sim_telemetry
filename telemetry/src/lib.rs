use std::{
    fmt::Display,
    fs::File,
    path::{Path, PathBuf},
    str::FromStr,
    thread::JoinHandle,
};

use crossbeam::channel::{bounded, unbounded, Receiver, Sender};
use csv::Writer;
pub use telemetry_facade::metric;
use telemetry_facade::recorder::*;

struct TelemetryRecorderImpl(Sender<Command>);

impl Recorder for TelemetryRecorderImpl {
    fn allocate(&self, name: &str, kind: MetricKind, unit: &str) -> MetricId {
        let (reply_tx, reply_rx) = bounded(1);
        let _ = self.0.send(Command::Register {
            name: name.into(),
            kind,
            unit: unit.into(),
            reply: reply_tx,
        });
        reply_rx.recv().unwrap_or(MetricId::MAX)
    }

    fn record_i64(&self, id: usize, value: i64) {
        let _ = self.0.send(Command::Update {
            metric: id,
            value: Value::Int64(value),
        });
    }

    fn record_u64(&self, id: usize, value: u64) {
        let _ = self.0.send(Command::Update {
            metric: id,
            value: Value::Uint64(value),
        });
    }

    fn record_f64(&self, id: usize, value: f64) {
        let _ = self.0.send(Command::Update {
            metric: id,
            value: Value::Float64(value),
        });
    }
}

#[derive(Debug, Clone)]
enum Command {
    Register {
        name: String,
        kind: MetricKind,
        unit: String,
        reply: Sender<MetricId>,
    },
    Timestamp(f64),
    Update {
        metric: MetricId,
        value: Value,
    },
    Exit,
}

#[derive(Debug, Clone)]
enum Value {
    Int64(i64),
    Uint64(u64),
    Float64(f64),
}

impl Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int64(v) => v.fmt(f),
            Self::Uint64(v) => v.fmt(f),
            Self::Float64(v) => v.fmt(f),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct TelemetryRecorder {
    handle: Option<JoinHandle<Result<(), TelemetryError>>>,
    cmd_tx: Sender<Command>,
}

impl TelemetryRecorder {
    pub fn new(filename: Option<&Path>) -> Result<Self, TelemetryError> {
        let filename = match filename {
            Some(filename) => filename.to_owned(),
            None => PathBuf::from_str("/tmp/out.csv").unwrap(),
        };

        let (cmd_tx, cmd_rx) = unbounded();
        let (start_tx, start_rx) = bounded(1);

        let handle = Some(Worker::spawn(filename, cmd_rx, start_tx));
        start_rx.recv().unwrap()?;
        telemetry_facade::set_recorder(TelemetryRecorderImpl(cmd_tx.clone()));
        Ok(Self { handle, cmd_tx })
    }

    pub fn timestamp_secs_f64(&self, time: f64) {
        let _ = self.cmd_tx.send(Command::Timestamp(time));
    }
}

impl Drop for TelemetryRecorder {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Command::Exit);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct Worker {
    writer: Writer<File>,
    cmd_rx: Receiver<Command>,
    metrics: Vec<Metric>,
    ts: f64,
    inflight: Vec<String>,
    has_data: bool,
    wrote_header: bool,
}

impl Worker {
    fn spawn(
        filename: PathBuf,
        cmd_rx: Receiver<Command>,
        start_result_tx: Sender<Result<(), TelemetryError>>,
    ) -> JoinHandle<Result<(), TelemetryError>> {
        std::thread::spawn(move || match Self::new(filename, cmd_rx) {
            Err(e) => {
                let _ = start_result_tx.send(Err(e));
                Ok(())
            }
            Ok(mut worker) => {
                let _ = start_result_tx.send(Ok(()));
                worker.run()
            }
        })
    }

    fn new(filename: PathBuf, cmd_rx: Receiver<Command>) -> Result<Self, TelemetryError> {
        let writer = Writer::from_path(filename)?;
        Ok(Self {
            writer,
            cmd_rx,
            metrics: vec![],
            ts: 0.0,
            inflight: vec![],
            has_data: false,
            wrote_header: false,
        })
    }

    fn run(&mut self) -> Result<(), TelemetryError> {
        while let Ok(msg) = self.cmd_rx.recv() {
            match msg {
                Command::Register {
                    name,
                    kind,
                    unit,
                    reply,
                } => {
                    let id = self.metrics.len();
                    self.metrics.push(Metric { name, kind, unit });
                    self.inflight.push("".into());
                    let _ = reply.send(id);
                }
                Command::Timestamp(ts) => {
                    self.ts = ts;
                    self.flush()?;
                }
                Command::Update { metric, value } => {
                    let Some(field) = self.inflight.get_mut(metric) else {
                        continue;
                    };
                    self.has_data = true;
                    *field = value.to_string();
                }
                Command::Exit => break,
            }
        }
        self.writer.flush()?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), TelemetryError> {
        if !self.has_data {
            return Ok(());
        }
        if !self.wrote_header {
            self.write_header()?;
            self.wrote_header = true;
        }
        self.write_data()
    }

    fn write_header(&mut self) -> Result<(), TelemetryError> {
        let header = std::iter::once("time").chain(self.metrics.iter().map(|m| m.name.as_str()));
        self.writer.write_record(header)?;
        Ok(())
    }

    fn write_data(&mut self) -> Result<(), TelemetryError> {
        let ts = self.ts.to_string();
        self.writer
            .write_record(std::iter::once(&ts).chain(self.inflight.iter()))?;
        Ok(())
    }
}

#[derive(Debug)]
struct Metric {
    name: String,
    #[expect(dead_code)]
    kind: MetricKind,
    #[expect(dead_code)]
    unit: String,
}
