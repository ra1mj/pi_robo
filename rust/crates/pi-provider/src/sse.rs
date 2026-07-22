use pi_model::ModelServiceError;

/// One decoded server-sent event before provider-specific interpretation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Incremental bounded SSE decoder supporting arbitrary byte boundaries.
#[derive(Debug)]
pub struct SseDecoder {
    line: Vec<u8>,
    pending_carriage_return: bool,
    event: Option<String>,
    data: Vec<String>,
    pending_event_bytes: usize,
    max_event_bytes: usize,
}

impl SseDecoder {
    #[must_use]
    pub fn new(max_event_bytes: usize) -> Self {
        Self {
            line: Vec::new(),
            pending_carriage_return: false,
            event: None,
            data: Vec::new(),
            pending_event_bytes: 0,
            max_event_bytes,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, ModelServiceError> {
        let mut events = Vec::new();
        for byte in chunk {
            if self.pending_carriage_return {
                self.pending_carriage_return = false;
                self.finish_line(&mut events)?;
                if *byte == b'\n' {
                    continue;
                }
            }
            match byte {
                b'\r' => self.pending_carriage_return = true,
                b'\n' => self.finish_line(&mut events)?,
                _ => {
                    self.line.push(*byte);
                    if self.pending_event_bytes.saturating_add(self.line.len())
                        > self.max_event_bytes
                    {
                        return Err(protocol_error(
                            "SSE event exceeded the configured byte limit",
                        ));
                    }
                }
            }
        }
        Ok(events)
    }

    /// Flushes a final unterminated line and trailing event at response EOF.
    pub fn finish(&mut self) -> Result<Vec<SseEvent>, ModelServiceError> {
        let mut events = Vec::new();
        if self.pending_carriage_return {
            self.pending_carriage_return = false;
            self.finish_line(&mut events)?;
        }
        if !self.line.is_empty() {
            self.finish_line(&mut events)?;
        }
        self.flush_event(&mut events);
        Ok(events)
    }

    fn finish_line(&mut self, events: &mut Vec<SseEvent>) -> Result<(), ModelServiceError> {
        let line = std::mem::take(&mut self.line);
        self.process_line(&line, events)
    }

    fn process_line(
        &mut self,
        line: &[u8],
        events: &mut Vec<SseEvent>,
    ) -> Result<(), ModelServiceError> {
        self.pending_event_bytes = self.pending_event_bytes.saturating_add(line.len());
        if self.pending_event_bytes > self.max_event_bytes {
            return Err(protocol_error(
                "SSE event exceeded the configured byte limit",
            ));
        }
        let line = std::str::from_utf8(line)
            .map_err(|_| protocol_error("SSE stream contained invalid UTF-8"))?;
        if line.is_empty() {
            self.flush_event(events);
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }

        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => self.event = Some(value.to_owned()),
            "data" => self.data.push(value.to_owned()),
            _ => {}
        }
        Ok(())
    }

    fn flush_event(&mut self, events: &mut Vec<SseEvent>) {
        if self.event.is_some() || !self.data.is_empty() {
            events.push(SseEvent {
                event: self.event.take(),
                data: self.data.join("\n"),
            });
        } else {
            self.event = None;
        }
        self.data.clear();
        self.pending_event_bytes = 0;
    }
}

fn protocol_error(message: &'static str) -> ModelServiceError {
    ModelServiceError::protocol(message, false)
}
