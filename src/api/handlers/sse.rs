use crate::AppState;
use axum::{
    extract::State,
    response::sse::{Event as SseEvent, KeepAlive, Sse},
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

pub(crate) async fn sse_events(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = state.event_hub.subscribe();

    let stream =
        BroadcastStream::new(rx).filter_map(
            |result: Result<crate::events::Event, _>| match result {
                Ok(event) => {
                    let event_type = event.event_type().to_string();
                    match serde_json::to_string(&event) {
                        Ok(json) => Some(Ok(SseEvent::default().event(event_type).data(json))),
                        Err(_) => None,
                    }
                }
                Err(_) => None,
            },
        );

    Sse::new(stream).keep_alive(KeepAlive::default())
}
