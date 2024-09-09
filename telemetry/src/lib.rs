use std::{
    path::{Path, PathBuf},
    str::FromStr,
    thread::JoinHandle,
};

use crossbeam::channel::{bounded, unbounded, Receiver, Sender};
use fstapi::{Handle, Writer};
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
    Timestamp(u64),
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

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("fst error: {0}")]
    FstApi(fstapi::Error),
}

impl From<fstapi::Error> for TelemetryError {
    fn from(value: fstapi::Error) -> Self {
        Self::FstApi(value)
    }
}

pub struct TelemetryRecorder {
    handle: Option<JoinHandle<Result<(), TelemetryError>>>,
    cmd_tx: Sender<Command>,
}

impl TelemetryRecorder {
    pub fn new(filename: Option<&Path>) -> Result<Self, TelemetryError> {
        let filename = match filename {
            Some(filename) => filename.to_owned(),
            None => PathBuf::from_str("/tmp/out.fst").unwrap(),
        };

        let (cmd_tx, cmd_rx) = unbounded();
        let (start_tx, start_rx) = bounded(1);

        let handle = Some(Worker::spawn(filename, cmd_rx, start_tx));
        start_rx.recv().unwrap()?;
        telemetry_facade::set_recorder(TelemetryRecorderImpl(cmd_tx.clone()));
        Ok(Self { handle, cmd_tx })
    }

    pub fn timestamp_secs_f64(&self, time: f64) {
        let ts = time * 1_000_000_000.0;
        let _ = self.cmd_tx.send(Command::Timestamp(ts as u64));
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
    writer: Writer,
    cmd_rx: Receiver<Command>,
    metrics: Vec<Metric>,
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
        let writer = Writer::create(filename, true)?.timescale_from_str("1ns")?;
        Ok(Self {
            writer,
            cmd_rx,
            metrics: vec![],
        })
    }

    fn run(&mut self) -> Result<(), TelemetryError> {
        use fstapi::var_dir;
        while let Ok(msg) = self.cmd_rx.recv() {
            match msg {
                Command::Register {
                    name,
                    kind,
                    unit: _unit,
                    reply,
                } => {
                    let id = self.metrics.len();

                    let var = self.writer.create_var(
                        Self::metric_type(kind),
                        var_dir::OUTPUT,
                        Self::metric_size(kind),
                        &name,
                        None,
                    )?;

                    self.metrics.push(Metric { var, kind });
                    let _ = reply.send(id);
                }
                Command::Timestamp(ts) => {
                    self.writer.emit_time_change(ts)?;
                }
                Command::Update { metric, value } => {
                    let Some(metric) = self.metrics.get(metric) else {
                        continue;
                    };
                    let (value, len) = Self::metric_value(metric.kind, value);
                    // println!("{metric:?} => {value:?}");
                    self.writer.emit_value_change(metric.var, &value[..len])?;
                }
                Command::Exit => break,
            }
        }
        self.writer.flush();
        println!("exit");
        Ok(())
    }

    fn metric_type(kind: MetricKind) -> u32 {
        use fstapi::var_type::*;
        match kind {
            MetricKind::Int8 => SV_BYTE,
            MetricKind::Int16 => SV_SHORTINT,
            MetricKind::Int32 => SV_INT,
            MetricKind::Int64 => SV_LONGINT,
            MetricKind::Uint8 => SV_BYTE,
            MetricKind::Uint16 => SV_SHORTINT,
            MetricKind::Uint32 => SV_INT,
            MetricKind::Uint64 => SV_LONGINT,
            MetricKind::Float32 => VCD_REAL,
            MetricKind::Float64 => VCD_REAL,
        }
    }

    fn metric_size(kind: MetricKind) -> u32 {
        match kind {
            MetricKind::Int8 => 1,
            MetricKind::Int16 => 8,
            MetricKind::Int32 => 8,
            MetricKind::Int64 => 8,
            MetricKind::Uint8 => 1,
            MetricKind::Uint16 => 8,
            MetricKind::Uint32 => 8,
            MetricKind::Uint64 => 8,
            MetricKind::Float32 => 8,
            MetricKind::Float64 => 8,
        }
    }

    fn metric_value(kind: MetricKind, value: Value) -> ([u8; 8], usize) {
        match (kind, value) {
            (MetricKind::Float64 | MetricKind::Float32, Value::Float64(v)) => (v.to_ne_bytes(), 8),
            _ => ([0u8; 8], 8),
        }
    }
}

#[derive(Debug)]
struct Metric {
    var: Handle,
    kind: MetricKind,
}
