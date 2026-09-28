use std;

// frame size (width*height)
pub(crate) struct Size {
    pub width: u32,
    pub height: u32,
}

pub type Char = char;
pub type Line = Vec<char>;

// a rectangle with data
pub(crate) struct Block {
    pub size: Size,
    pub data: Vec<Line>,
}

// full frame with data
pub(crate) struct Frame {

}

// region with multi blocks and data
pub(crate) struct Region {

}

pub(crate) struct UiWaker {

}

// all events that have removed platform relevance
pub(crate) enum UiEvent {

}

// the ability of Backend to be transparently blocked
pub(crate) enum UiCapability {

}

pub(crate) type UiCapabilities = Vec<UiCapability>;

pub trait UiBackend {
    type Error: std::error::Error + Send + Sync + 'static;

    /// UI session lifecycle
    fn enter(&mut self) -> Result<(), Self::Error>;
    fn leave(&mut self) -> Result<(), Self::Error>;

    /// Current status
    fn size(&self) -> Size;
    fn current_frame(&self) -> Result<Frame, Self::Error>;

    /// Capabilities of spec backend
    fn capabilities(&self) -> &UiCapabilities;

    /// Submit update of frame
    fn present(
        &mut self,
        frame: &Frame,
    ) -> Result<(), Self::Error>;
    fn update(
        &mut self,
        region: &Region,
    ) -> Result<(), Self::Error>;

    /// wait for an event with timeout, None for wait infinity
    fn poll_event(
        &mut self,
        timeout: Option<std::time::Duration>,
    ) -> Result<Option<UiEvent>, Self::Error>;

    /// wake up poll_event from other thread/task
    fn waker(&self) -> UiWaker;
}