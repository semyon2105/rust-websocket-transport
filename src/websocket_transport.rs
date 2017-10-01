use std::collections::VecDeque;
use std::convert::{TryFrom, TryInto};
use websocket::{OwnedMessage, WebSocketError};
use websocket::async::futures::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransportOwnedMessage {
    Binary(Vec<u8>),
    Text(String),
}

impl From<Vec<u8>> for TransportOwnedMessage {
    fn from(data: Vec<u8>) -> TransportOwnedMessage {
        TransportOwnedMessage::Binary(data)
    }
}

impl From<String> for TransportOwnedMessage {
    fn from(text: String) -> TransportOwnedMessage {
        TransportOwnedMessage::Text(text)
    }
}

impl TryFrom<OwnedMessage> for TransportOwnedMessage {
    type Error = OwnedMessage;

    fn try_from(item: OwnedMessage) -> Result<TransportOwnedMessage, OwnedMessage> {
        match item {
            OwnedMessage::Binary(data) => Ok(TransportOwnedMessage::Binary(data)),
            OwnedMessage::Text(text) => Ok(TransportOwnedMessage::Text(text)),
            other => Err(other),
        }
    }
}

impl From<TransportOwnedMessage> for OwnedMessage {
    fn from(item: TransportOwnedMessage) -> OwnedMessage {
        match item {
            TransportOwnedMessage::Binary(data) => OwnedMessage::Binary(data),
            TransportOwnedMessage::Text(text) => OwnedMessage::Text(text),
        }
    }
}

#[derive(Debug)]
pub enum TransportError {
    WebSocketError(WebSocketError),
}

impl From<WebSocketError> for TransportError {
    fn from(err: WebSocketError) -> Self {
        TransportError::WebSocketError(err)
    }
}

pub struct WebSocketTransport<T> {
    pings: VecDeque<Vec<u8>>,
    upstream: T,
}

impl<T> From<T> for WebSocketTransport<T>
where
    T: Stream<Item = OwnedMessage, Error = WebSocketError>,
    T: Sink<SinkItem = OwnedMessage, SinkError = WebSocketError>,
{
    fn from(transport: T) -> WebSocketTransport<T> {
        WebSocketTransport {
            pings: VecDeque::new(),
            upstream: transport,
        }
    }
}

impl<T> Stream for WebSocketTransport<T>
where
    T: Stream<Item = OwnedMessage, Error = WebSocketError>,
    T: Sink<SinkItem = OwnedMessage, SinkError = WebSocketError>,
{
    type Item = TransportOwnedMessage;
    type Error = TransportError;

    fn poll(&mut self) -> Poll<Option<TransportOwnedMessage>, TransportError> {
        loop {
            match try_ready!(self.upstream.poll()) {
                Some(OwnedMessage::Ping(data)) => {
                    self.pings.push_back(data);
                    self.poll_complete()?;
                }
                Some(OwnedMessage::Pong(_)) => {
                    self.poll_complete()?;
                }
                Some(OwnedMessage::Close(_)) => {
                    self.close()?;
                }
                Some(owned_msg) => {
                    // Only two cases of owned message left: Binary and Text.
                    // unwrap() cannot panic
                    let msg: TransportOwnedMessage = owned_msg.try_into().unwrap();
                    return Ok(Async::Ready(msg.into()));
                }
                None => return Ok(Async::Ready(None)),
            }
        }
    }
}

impl<T> Sink for WebSocketTransport<T>
where
    T: Sink<SinkItem = OwnedMessage, SinkError = WebSocketError>,
{
    type SinkItem = TransportOwnedMessage;
    type SinkError = TransportError;

    fn start_send(&mut self, item: TransportOwnedMessage) -> StartSend<TransportOwnedMessage, TransportError> {
        if self.pings.len() != 0 {
            return Ok(AsyncSink::NotReady(item));
        }

        self.upstream
            .start_send(item.into())
            .map(|async_sink| {
                async_sink.map(|owned_msg| owned_msg.try_into().unwrap())
            })
            .map_err(|ws_error| ws_error.into())
    }

    fn poll_complete(&mut self) -> Poll<(), TransportError> {
        while let Some(data) = self.pings.pop_front() {
            let result = self.upstream.start_send(OwnedMessage::Pong(data))?;
            if let AsyncSink::NotReady(OwnedMessage::Pong(data)) = result {
                self.pings.push_front(data);
                break;
            }
        }

        self.upstream
            .poll_complete()
            .map_err(|ws_error| ws_error.into())
    }

    fn close(&mut self) -> Poll<(), TransportError> {
        self.upstream
            .poll_complete()
            .map_err(|ws_error| ws_error.into())
    }
}
