//! Agent supervision helpers: bounded diagnostic trace buffer.

use std::collections::VecDeque;

/// Fixed-capacity line ring buffer for agent stderr and protocol traces.
#[derive(Debug, Clone)]
pub struct RingBuffer {
    capacity: usize,
    lines: VecDeque<String>,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            lines: VecDeque::new(),
        }
    }

    pub fn push(&mut self, line: impl Into<String>) {
        if self.lines.len() >= self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line.into());
    }

    pub fn lines(&self) -> &VecDeque<String> {
        &self.lines
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

/// Supervisor state shown in the ACP Trace bottom tab.
#[derive(Debug, Clone)]
pub struct AcpSupervisor {
    trace: RingBuffer,
}

impl Default for AcpSupervisor {
    fn default() -> Self {
        Self::new(500)
    }
}

impl AcpSupervisor {
    pub fn new(capacity: usize) -> Self {
        Self {
            trace: RingBuffer::new(capacity),
        }
    }

    pub fn record(&mut self, line: impl Into<String>) {
        self.trace.push(line);
    }

    pub fn trace(&self) -> &RingBuffer {
        &self.trace
    }

    pub fn clear(&mut self) {
        self.trace.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_keeps_latest_lines() {
        let mut buffer = RingBuffer::new(3);
        for index in 0..5 {
            buffer.push(format!("line {index}"));
        }
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.lines().front().map(String::as_str), Some("line 2"));
        assert_eq!(buffer.lines().back().map(String::as_str), Some("line 4"));
    }
}
