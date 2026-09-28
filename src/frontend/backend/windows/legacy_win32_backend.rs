use std::time::Duration;
use crate::frontend::backend::ui_backend::{Frame, Region, Size, UiCapabilities, UiEvent, UiWaker};
use super::super::ui_backend::UiBackend;

pub(crate) struct LegacyWin32Backend;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("backend initialization failed")]
    InitializationFailed,
}

impl UiBackend for LegacyWin32Backend {
    type Error = BackendError;

    fn enter(&mut self) -> Result<(), Self::Error> {
        todo!()
    }

    fn leave(&mut self) -> Result<(), Self::Error> {
        todo!()
    }

    fn size(&self) -> Size {
        todo!()
    }

    fn current_frame(&self) -> Result<Frame, Self::Error> {
        todo!()
    }

    fn capabilities(&self) -> &UiCapabilities {
        todo!()
    }

    fn present(&mut self, frame: &Frame) -> Result<(), Self::Error> {
        todo!()
    }

    fn update(&mut self, region: &Region) -> Result<(), Self::Error> {
        todo!()
    }

    fn poll_event(&mut self, timeout: Option<Duration>) -> Result<Option<UiEvent>, Self::Error> {
        todo!()
    }

    fn waker(&self) -> UiWaker {
        todo!()
    }
}