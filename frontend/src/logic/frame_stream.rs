use bh_types::{
    ErrorCode,
    ipc::{CloseReason, ErrorEnvelope, StreamFrame},
};

use crate::logic::channel_order::{Delivery, OrderedDelivery, Packet};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StreamStep {
    pub frames: Vec<StreamFrame>,
    pub ended: bool,
    pub wait_for_gap: bool,
}

#[derive(Debug, Default)]
pub struct FrameStream {
    order: OrderedDelivery<StreamFrame>,
    closed: bool,
}

impl FrameStream {
    pub fn accept(&mut self, index: u64, packet: Packet<StreamFrame>) -> StreamStep {
        let delivery = self.order.accept(index, packet);
        let mut step = self.deliver(delivery);
        step.wait_for_gap = self.order.end_is_waiting();
        step
    }

    pub fn malformed(&mut self) -> StreamStep {
        let delivery = self.order.flush_past_gaps();
        self.deliver(delivery)
    }

    pub fn gap_expired(&mut self) -> StreamStep {
        if !self.order.end_is_waiting() {
            return StreamStep::default();
        }
        let delivery = self.order.flush_past_gaps();
        self.deliver(delivery)
    }

    fn deliver(&mut self, delivery: Delivery<StreamFrame>) -> StreamStep {
        let mut step = StreamStep {
            frames: Vec::new(),
            ended: delivery.ended,
            wait_for_gap: false,
        };
        for frame in delivery.messages {
            if self.closed {
                break;
            }
            self.closed = matches!(frame, StreamFrame::Closed { .. });
            step.frames.push(frame);
        }
        if delivery.ended {
            self.close_if_open(&mut step);
        }
        step
    }

    fn close_if_open(&mut self, step: &mut StreamStep) {
        if self.closed {
            return;
        }
        self.closed = true;
        step.frames.push(StreamFrame::Closed {
            reason: CloseReason::Failed {
                error: interrupted(),
            },
        });
    }
}

pub fn interrupted() -> ErrorEnvelope {
    ErrorEnvelope {
        code: ErrorCode::TruncatedStream,
        message: "The response stream was interrupted. Send again.".to_owned(),
        retryable: false,
        user_action: None,
        request_id: None,
        correlation_id: "stream".to_owned(),
    }
}
