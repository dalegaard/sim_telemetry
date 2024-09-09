use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};

pub trait Recorder: Send + Sync {
    fn allocate(&self, name: &str, kind: MetricKind, unit: &str) -> MetricId;

    fn record_i8(&self, id: usize, value: u8) {
        self.record_i64(id, value.into());
    }
    fn record_i16(&self, id: usize, value: i16) {
        self.record_i64(id, value.into());
    }
    fn record_i32(&self, id: usize, value: i32) {
        self.record_i64(id, value.into());
    }
    fn record_i64(&self, id: usize, value: i64);
    fn record_u8(&self, id: usize, value: u8) {
        self.record_u64(id, value.into());
    }
    fn record_u16(&self, id: usize, value: u16) {
        self.record_u64(id, value.into());
    }
    fn record_u32(&self, id: usize, value: u32) {
        self.record_u64(id, value.into());
    }
    fn record_u64(&self, id: usize, value: u64);
    fn record_f32(&self, id: usize, value: f32) {
        self.record_f64(id, value.into());
    }
    fn record_f64(&self, id: usize, value: f64);
}

pub struct RecorderCell {
    recorder: UnsafeCell<Option<&'static dyn Recorder>>,
    state: AtomicUsize,
}

unsafe impl Send for RecorderCell {}
unsafe impl Sync for RecorderCell {}

impl RecorderCell {
    const fn new() -> Self {
        Self {
            recorder: UnsafeCell::new(None),
            state: AtomicUsize::new(0),
        }
    }

    pub fn set<R>(&self, recorder: R) -> bool
    where
        R: Recorder + 'static,
    {
        if self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            unsafe {
                self.recorder
                    .get()
                    .write(Some(Box::leak(Box::new(recorder))));
            }
            self.state.store(2, Ordering::Release);
            true
        } else {
            false
        }
    }

    pub fn get(&self) -> Option<&'static dyn Recorder> {
        if self.state.load(Ordering::Acquire) != 2 {
            None
        } else {
            unsafe { self.recorder.get().read() }
        }
    }
}

pub static RECORDER: RecorderCell = RecorderCell::new();

pub type MetricId = usize;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum MetricKind {
    Int8,
    Int16,
    Int32,
    Int64,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Float32,
    Float64,
}

#[macro_export]
macro_rules! __metric_kind {
    (Int8) => {
        $crate::recorder::MetricKind::Int8
    };
    (Int16) => {
        $crate::recorder::MetricKind::Int16
    };
    (Int32) => {
        $crate::recorder::MetricKind::Int32
    };
    (Int64) => {
        $crate::recorder::MetricKind::Int64
    };
    (Uint8) => {
        $crate::recorder::MetricKind::Uint8
    };
    (Uint16) => {
        $crate::recorder::MetricKind::Uint16
    };
    (Uint32) => {
        $crate::recorder::MetricKind::Uint32
    };
    (Uint64) => {
        $crate::recorder::MetricKind::Uint64
    };
    (f32) => {
        $crate::recorder::MetricKind::Float32
    };
    (f64) => {
        $crate::recorder::MetricKind::Float64
    };
    ($type:ident) => {
        compile_error!(concat!("Unsupported metric kind ", stringify!($type)));
    };
}

pub use __metric_kind;
