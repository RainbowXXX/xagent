use std::time::Duration;

use anyhow;

use crate::frontend::backend::ui_backend;
use crate::frontend::backend::ui_backend::UiBackend;

pub(crate) struct VTBackend;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("backend initialization failed")]
    InitializationFailed,
}

impl UiBackend for VTBackend {
    type Error = BackendError;

    fn enter(&mut self) -> Result<(), Self::Error> {
        todo!()
    }

    fn leave(&mut self) -> Result<(), Self::Error> {
        todo!()
    }

    fn size(&self) -> ui_backend::Size {
        todo!()
    }

    fn current_frame(&self) -> Result<ui_backend::Frame, Self::Error> {
        todo!()
    }

    fn capabilities(&self) -> &ui_backend::UiCapabilities {
        todo!()
    }

    fn present(&mut self, frame: &ui_backend::Frame) -> Result<(), Self::Error> {
        todo!()
    }

    fn update(&mut self, region: &ui_backend::Region) -> Result<(), Self::Error> {
        todo!()
    }

    fn poll_event(&mut self, timeout: Option<Duration>) -> Result<Option<ui_backend::UiEvent>, Self::Error> {
        todo!()
    }

    fn waker(&self) -> ui_backend::UiWaker {
        todo!()
    }
}