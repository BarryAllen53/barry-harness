use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet<T> {
    Message(T),
    End,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Delivery<T> {
    pub messages: Vec<T>,
    pub ended: bool,
}

#[derive(Debug)]
pub struct OrderedDelivery<T> {
    next_index: u64,
    waiting: BTreeMap<u64, Packet<T>>,
    ended: bool,
}

impl<T> Default for OrderedDelivery<T> {
    fn default() -> Self {
        Self {
            next_index: 0,
            waiting: BTreeMap::new(),
            ended: false,
        }
    }
}

impl<T> OrderedDelivery<T> {
    pub fn accept(&mut self, index: u64, packet: Packet<T>) -> Delivery<T> {
        let mut messages = Vec::new();
        if !self.ended && index >= self.next_index {
            self.waiting.insert(index, packet);
        }
        while !self.ended {
            let Some(packet) = self.waiting.remove(&self.next_index) else {
                break;
            };
            self.next_index += 1;
            match packet {
                Packet::Message(message) => messages.push(message),
                Packet::End => {
                    self.ended = true;
                    self.waiting.clear();
                }
            }
        }
        Delivery {
            messages,
            ended: self.ended,
        }
    }

    pub fn end_is_waiting(&self) -> bool {
        !self.ended
            && self
                .waiting
                .values()
                .any(|packet| matches!(packet, Packet::End))
    }

    pub fn flush_past_gaps(&mut self) -> Delivery<T> {
        let messages = if self.ended {
            Vec::new()
        } else {
            std::mem::take(&mut self.waiting)
                .into_values()
                .filter_map(|packet| match packet {
                    Packet::Message(message) => Some(message),
                    Packet::End => None,
                })
                .collect()
        };
        self.ended = true;
        Delivery {
            messages,
            ended: true,
        }
    }
}
