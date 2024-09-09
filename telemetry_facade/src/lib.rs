#![cfg_attr(not(feature = "enable"), no_std)]

#[doc(hidden)]
pub use paste::paste;

#[cfg(feature = "enable")]
#[macro_export]
macro_rules! metric {
    ($name:ident [$unit:literal] : $type:ident = $value:expr) => {{
        static ID: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        if let Some(recorder) = $crate::recorder::RECORDER.get() {
            let id = *ID.get_or_init(|| {
                recorder.allocate(
                    stringify!($name),
                    $crate::recorder::__metric_kind!($type),
                    $unit,
                )
            });
            $crate::paste! {
                recorder.[<record_ $type>](id, $value);
            }
        }
    }};
}

#[cfg(not(feature = "enable"))]
#[macro_export]
macro_rules! metric {
    ($name:ident [$unit:literal] : $type:ty = $value:expr, [unit:lit]) => {};
}

#[cfg(feature = "enable")]
#[doc(hidden)]
pub mod recorder;

#[cfg(feature = "enable")]
pub fn set_recorder<R>(recorder: R) -> bool
where
    R: recorder::Recorder + 'static,
{
    recorder::RECORDER.set(recorder)
}
