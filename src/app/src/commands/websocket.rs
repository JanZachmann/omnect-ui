//! WebSocket command definitions.
//!
//! These types define the interface between the Core and the Shell for WebSocket operations.

use crate::types::websocket::WebSocketChannel;
use crux_core::{Command, capability::Operation, command};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

// Operations that the Shell needs to perform for WebSocket
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WebSocketOperation {
    Connect,
    Disconnect,
    Subscribe { channel: WebSocketChannel },
    Unsubscribe { channel: WebSocketChannel },
    SubscribeAll,
    UnsubscribeAll,
    History { channel: WebSocketChannel },
}

// The output from WebSocket operations (shell tells us what happened)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WebSocketOutput {
    Connected,
    Disconnected,
    Subscribed {
        channel: WebSocketChannel,
    },
    Unsubscribed {
        channel: WebSocketChannel,
    },
    Message {
        channel: WebSocketChannel,
        data: String,
    },
    HistoryResult {
        channel: WebSocketChannel,
        data: Option<String>,
    },
    Error {
        message: String,
    },
}

impl Operation for WebSocketOperation {
    type Output = WebSocketOutput;
}

/// Command-based WebSocket API
pub struct WebSocket<Effect, Event> {
    _effect: PhantomData<Effect>,
    _event: PhantomData<Event>,
}

impl<Effect, Event> WebSocket<Effect, Event>
where
    Effect: Send + From<crux_core::Request<WebSocketOperation>> + 'static,
    Event: Send + 'static,
{
    /// Connect to WebSocket server
    pub fn connect() -> RequestBuilder<Effect, Event> {
        RequestBuilder::new(WebSocketOperation::Connect)
    }

    /// Disconnect from WebSocket server
    pub fn disconnect() -> RequestBuilder<Effect, Event> {
        RequestBuilder::new(WebSocketOperation::Disconnect)
    }

    /// Subscribe to a specific channel
    pub fn subscribe(channel: WebSocketChannel) -> RequestBuilder<Effect, Event> {
        RequestBuilder::new(WebSocketOperation::Subscribe { channel })
    }

    /// Unsubscribe from a specific channel
    pub fn unsubscribe(channel: WebSocketChannel) -> RequestBuilder<Effect, Event> {
        RequestBuilder::new(WebSocketOperation::Unsubscribe { channel })
    }

    /// Subscribe to all known channels
    pub fn subscribe_all() -> RequestBuilder<Effect, Event> {
        RequestBuilder::new(WebSocketOperation::SubscribeAll)
    }

    /// Unsubscribe from all channels
    pub fn unsubscribe_all() -> RequestBuilder<Effect, Event> {
        RequestBuilder::new(WebSocketOperation::UnsubscribeAll)
    }

    /// Get history (last message) from a channel
    pub fn history(channel: WebSocketChannel) -> RequestBuilder<Effect, Event> {
        RequestBuilder::new(WebSocketOperation::History { channel })
    }
}

/// Request builder for WebSocket operations
#[must_use]
pub struct RequestBuilder<Effect, Event> {
    operation: WebSocketOperation,
    _effect: PhantomData<Effect>,
    _event: PhantomData<fn() -> Event>,
}

impl<Effect, Event> RequestBuilder<Effect, Event>
where
    Effect: Send + From<crux_core::Request<WebSocketOperation>> + 'static,
    Event: Send + 'static,
{
    fn new(operation: WebSocketOperation) -> Self {
        Self {
            operation,
            _effect: PhantomData,
            _event: PhantomData,
        }
    }

    /// Build the request into a Command `RequestBuilder`
    #[must_use]
    pub fn build(
        self,
    ) -> command::RequestBuilder<Effect, Event, impl Future<Output = WebSocketOutput>> {
        command::RequestBuilder::new(move |ctx| async move {
            Command::request_from_shell(self.operation)
                .into_future(ctx)
                .await
        })
    }
}
