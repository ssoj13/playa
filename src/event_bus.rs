//! Hybrid event bus: pub/sub callbacks + event queue for deferred processing.
//!
//! Supports two patterns:
//! 1. Pub/sub: subscribe() + emit() for immediate callback invocation
//! 2. Queue: send() + drain() for deferred batch processing (egui-friendly)

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// Marker trait for events. Events must be Send + Sync + 'static.
pub trait Event: Any + Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
    fn type_name(&self) -> &'static str;
}

// Blanket impl for all qualifying types
impl<T: Any + Send + Sync + 'static> Event for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

/// Type-erased callback
type Callback = Arc<dyn Fn(&dyn Any) + Send + Sync>;

/// Boxed event for queue storage
pub type BoxedEvent = Box<dyn Event>;

/// Hybrid event bus: pub/sub + queue.
#[derive(Clone)]
pub struct EventBus {
    subscribers: Arc<RwLock<HashMap<TypeId, Vec<Callback>>>>,
    queue: Arc<Mutex<Vec<BoxedEvent>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    // ========== Pub/Sub ==========

    /// Subscribe to events of type E
    pub fn subscribe<E, F>(&self, callback: F)
    where
        E: Event,
        F: Fn(&E) + Send + Sync + 'static,
    {
        let type_id = TypeId::of::<E>();
        let wrapped: Callback = Arc::new(move |any: &dyn Any| {
            if let Some(event) = any.downcast_ref::<E>() {
                callback(event);
            }
        });
        self.subscribers
            .write()
            .expect("lock")
            .entry(type_id)
            .or_default()
            .push(wrapped);
    }

    /// Emit event: invoke subscribers immediately
    pub fn emit<E: Event>(&self, event: E) {
        let type_id = TypeId::of::<E>();
        if let Some(cbs) = self.subscribers.read().expect("lock").get(&type_id) {
            for cb in cbs {
                cb(&event);
            }
        }
    }

    // ========== Queue ==========

    /// Send event to queue
    pub fn send<E: Event>(&self, event: E) {
        self.queue.lock().expect("lock").push(Box::new(event));
    }

    /// Drain all queued events
    pub fn drain(&self) -> Vec<BoxedEvent> {
        std::mem::take(&mut *self.queue.lock().expect("lock"))
    }

    /// Get sender handle
    pub fn sender(&self) -> EventSender {
        EventSender {
            queue: Arc::clone(&self.queue),
        }
    }

    /// Clear subscribers for type E
    pub fn clear<E: Event>(&self) {
        self.subscribers.write().expect("lock").remove(&TypeId::of::<E>());
    }

    /// Clear all
    pub fn clear_all(&self) {
        self.subscribers.write().expect("lock").clear();
        self.queue.lock().expect("lock").clear();
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Sender handle for UI components
#[derive(Clone)]
pub struct EventSender {
    queue: Arc<Mutex<Vec<BoxedEvent>>>,
}

impl EventSender {
    pub fn send<E: Event>(&self, event: E) {
        self.queue.lock().expect("lock").push(Box::new(event));
    }
}

/// Comp-specific event sender (wraps Option<EventSender>)
#[derive(Clone, Default)]
pub struct CompEventSender {
    inner: Option<EventSender>,
}

impl CompEventSender {
    /// Create a no-op sender
    pub fn dummy() -> Self {
        Self { inner: None }
    }

    /// Create from EventSender
    pub fn from_sender(sender: EventSender) -> Self {
        Self { inner: Some(sender) }
    }

    /// Send any event
    pub fn send<E: Event>(&self, event: E) {
        if let Some(ref sender) = self.inner {
            sender.send(event);
        }
    }
}

/// Helper: downcast BoxedEvent to concrete type
#[inline]
pub fn downcast_event<E: Event>(event: &BoxedEvent) -> Option<&E> {
    event.as_any().downcast_ref::<E>()
}

/// Macro for matching events in drain loop
#[macro_export]
macro_rules! match_event {
    ($event:expr, $($type:ty => $handler:expr),+ $(,)?) => {{
        let ev = &$event;
        $(
            if let Some(e) = $crate::event_bus::downcast_event::<$type>(ev) {
                $handler(e);
            } else
        )+
        {}
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};

    #[derive(Clone, Debug)]
    struct TestEvent { value: i32 }

    #[derive(Clone, Debug)]
    struct OtherEvent;

    #[test]
    fn test_pub_sub() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicI32::new(0));
        let c = counter.clone();
        bus.subscribe(move |e: &TestEvent| {
            c.fetch_add(e.value, Ordering::SeqCst);
        });
        bus.emit(TestEvent { value: 10 });
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn test_queue() {
        let bus = EventBus::new();
        bus.send(TestEvent { value: 1 });
        bus.send(OtherEvent);
        assert_eq!(bus.drain().len(), 2);
    }

    #[test]
    fn test_downcast() {
        let bus = EventBus::new();
        bus.send(TestEvent { value: 42 });
        for ev in bus.drain() {
            if let Some(e) = downcast_event::<TestEvent>(&ev) {
                assert_eq!(e.value, 42);
            }
        }
    }
}
