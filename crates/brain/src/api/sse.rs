use super::*;

// ---------------------------------------------------------------------------------------------
// SSE
// ---------------------------------------------------------------------------------------------
#[derive(Deserialize)]
pub(super) struct EventsQuery {
    #[serde(default)]
    after: u64,
    #[serde(default = "default_follow")]
    follow: bool,
    /// Optional exact strong replay boundary. Finite billing/audit consumers capture this from
    /// GET Session and require the matching replay.complete proof before installing it.
    through: Option<u64>,
}
fn default_follow() -> bool {
    true
}

pub(super) async fn stream_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<SseFrame, Infallible>>>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    // Last-Event-ID (reconnect) wins over ?after. A present-but-malformed resume cursor is a
    // request error: silently replaying from ?after would hand the client the wrong window.
    let after = match headers.get("last-event-id") {
        None => q.after,
        Some(value) => value
            .to_str()
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| {
                Failure(
                    StatusCode::BAD_REQUEST,
                    api_code("invalid_request"),
                    "Last-Event-ID must be a decimal event seq".into(),
                )
            })?,
    };

    // Existence check first: a stream for a missing session must 404, not hang.
    let head = state.brain.head(&id).await.map_err(map_err)?;
    if head.doc.state == SessionLifecycle::Deleted {
        return Err(Failure(
            StatusCode::NOT_FOUND,
            api_code("not_found"),
            "session deleted".into(),
        ));
    }
    if q.follow && q.through.is_some() {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            "through is valid only when follow=false".into(),
        ));
    }
    if q.through.is_some_and(|through| through > head.last_seq) {
        return Err(Failure(
            StatusCode::CONFLICT,
            api_code("conflict"),
            "requested event replay boundary is ahead of the authoritative session high-water"
                .into(),
        ));
    }
    if q.through.is_some_and(|through| after > through) {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            "event replay cursor is ahead of the requested boundary".into(),
        ));
    }

    let brain = state.brain.clone();
    let follow = q.follow;
    let requested_through = q.through;
    // Admission happens before response headers are committed. A finite replay needs no live
    // ring; a followed stream holds its process-wide permit until the response body is dropped.
    let subscription = if follow {
        Some(
            brain
                .hub
                .subscribe(&id)
                .map_err(|_| map_err(BrainError::Overloaded))?,
        )
    } else {
        None
    };
    let stream = async_stream::stream! {
        // Subscribe BEFORE capturing the fixed replay high-water so no event falls between the
        // strong HEAD read and the live tail.
        let snapshot = match brain.journal.get_head(&id).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(session = %id, error = %error, "event replay snapshot failed");
                return;
            }
        };
        let through_seq = requested_through.unwrap_or(snapshot.last_seq);
        let mut last = after;

        while last < through_seq {
            let page = match brain.journal.read_record_page(&crate::journal::RecordPageQuery {
                session_id: &id,
                after: last,
                through_seq,
                limit: crate::journal::DEFAULT_RECORD_PAGE_ITEMS,
                max_bytes: crate::journal::DEFAULT_RECORD_PAGE_BYTES,
            }).await {
                Ok(page) => page,
                Err(error) => {
                    tracing::warn!(session = %id, error = %error, "replay page failed");
                    // Never tail live after an incomplete durable replay. Ending without
                    // advancing Last-Event-ID makes EventSource retry from the last confirmed
                    // record.
                    return;
                }
            };
            for entry in &page.entries {
                if let Some(event) =
                    crate::events::derive(&id, entry.seq, entry.ts_ms, &entry.record)
                {
                    let Some(frame) = frame(&event, true) else {
                        return;
                    };
                    yield Ok(frame);
                }
                // Internal-only records still advance the page cursor; only emitted durable
                // events update a client's Last-Event-ID.
                last = last.max(entry.seq);
            }
            let Some(next) = page.next_after else {
                break;
            };
            last = next;
        }
        // Sequence gaps through the snapshot are live-only provisional events. They are not
        // replayable, but queued live frames at or below the snapshot must be deduplicated.
        last = last.max(through_seq);

        let completion = session::Event::ReplayComplete {
            session_id: match id.parse() {
                Ok(session_id) => session_id,
                Err(_) => return,
            },
            through_seq,
        };
        let Some(completion) = frame(&completion, false) else {
            return;
        };
        yield Ok(completion);

        if !follow {
            return;
        }
        let Some(mut rx) = subscription else {
            return;
        };
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let seq = event_seq(&ev);
                    if seq <= last {
                        continue; // replayed already
                    }
                    let durable = !event_is_ephemeral(&ev);
                    if durable {
                        last = seq;
                    }
                    let stop = matches!(&*ev, session::Event::SessionUpdated { state, .. }
                        if *state == session::SessionState::Deleted);
                    let Some(frame) = frame(&ev, durable) else {
                        return;
                    };
                    yield Ok(frame);
                    if stop {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(session = %id, lagged = n, "sse consumer lagged; events skipped");
                    // The skipped range may contain durable records. End the stream without
                    // advancing its cursor; EventSource reconnects from the last delivered
                    // durable id and journal replay fills the exact gap.
                    return;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn frame(ev: &session::Event, durable: bool) -> Option<SseFrame> {
    let data = serde_json::to_string(ev).ok()?;
    if data.len() > brain_protocol::MAX_PUBLIC_EVENT_BYTES {
        tracing::error!(
            event = event_type(ev),
            bytes = data.len(),
            limit = brain_protocol::MAX_PUBLIC_EVENT_BYTES,
            "public event exceeded its canonical byte ceiling"
        );
        return None;
    }
    // serde_json escapes embedded newlines, so every public event is exactly one `data:` line.
    let frame = SseFrame::default().event(event_type(ev)).data(data);
    if durable {
        Some(frame.id(event_seq(ev).to_string()))
    } else {
        Some(frame)
    }
}

// Re-exported for the M0 gate binary: build one deterministic replay of a session's durable
// events (no follow), used to assert byte-stable replay.
pub async fn replay(
    brain: &Arc<Brain>,
    session_id: &str,
    after: u64,
) -> crate::Result<Vec<session::Event>> {
    let head = brain.journal.get_head(session_id).await?;
    let mut out = Vec::new();
    let mut cursor = after;
    while cursor < head.last_seq {
        let page = brain
            .journal
            .read_record_page(&crate::journal::RecordPageQuery {
                session_id,
                after: cursor,
                through_seq: head.last_seq,
                limit: crate::journal::DEFAULT_RECORD_PAGE_ITEMS,
                max_bytes: crate::journal::DEFAULT_RECORD_PAGE_BYTES,
            })
            .await?;
        for entry in &page.entries {
            if let Some(event) =
                crate::events::derive(session_id, entry.seq, entry.ts_ms, &entry.record)
            {
                out.push(event);
            }
            cursor = cursor.max(entry.seq);
        }
        let Some(next) = page.next_after else {
            break;
        };
        cursor = next;
    }
    let _ = Record::TurnStarted {
        turn: String::new(),
    }; // keep the import honest
    Ok(out)
}
