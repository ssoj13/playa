//! Pub/Sub Event Bus for decoupled component communication.
//!
//! Architecture:
//! - Components subscribe to event types with callbacks (immediate invocation)
//! - emit() invokes callbacks immediately AND queues for deferred processing
//! - poll() returns queued events for batch processing in main loop
//!
//! Callback order: FIFO (first-subscribed, first-called) within same event type.
//! Cross-type order undefined - don't rely on ordering between different event types.
//!
//! This provides true pub/sub with egui-friendly deferred processing.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use log::warn;

/// Maximum events in queue before oldest are evicted
const MAX_QUEUE_SIZE: usize = 1000;

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

/// Pub/Sub Event Bus with deferred processing support.
///
/// Two modes of operation:
/// 1. Immediate: subscribe() + emit() triggers callbacks instantly
/// 2. Deferred: emit() also queues events for poll() in main loop
///
/// Both modes work together - callbacks fire immediately, and events
/// are also available for batch processing via poll().
#[derive(Clone)]
pub struct EventBus {
    subscribers: Arc<RwLock<HashMap<TypeId, Vec<Callback>>>>,
    queue: Arc<Mutex<Vec<BoxedEvent>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    // ========== Pub/Sub (immediate) ==========

    /// Subscribe to events of type E.
    ///
    /// Callback is invoked immediately when emit() is called.
    /// Use Arc<Mutex<State>> in the callback for state mutations.
    ///
    /// # Example
    /// ```ignore
    /// let state = Arc::new(Mutex::new(MyState::default()));
    /// let state_clone = Arc::clone(&state);
    /// event_bus.subscribe::<MyEvent, _>(move |e| {
    ///     state_clone.lock().unwrap().handle(e);
    /// });
    /// ```
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
            .unwrap_or_else(|e| e.into_inner())
            .entry(type_id)
            .or_default()
            .push(wrapped);
    }

    /// Emit event: invoke callbacks immediately AND queue for deferred processing.
    ///
    /// Callbacks are called synchronously, then event is added to queue
    /// for retrieval via poll().
    pub fn emit<E: Event + Clone>(&self, event: E) {
        let type_id = TypeId::of::<E>();

        // Invoke immediate callbacks
        if let Some(cbs) = self.subscribers.read().unwrap_or_else(|e| e.into_inner()).get(&type_id) {
            for cb in cbs {
                cb(&event);
            }
        }

        // Queue for deferred processing with eviction
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.len() >= MAX_QUEUE_SIZE {
            let evict_count = queue.len() / 2;
            warn!("EventBus queue full ({} events), evicting oldest {}", queue.len(), evict_count);
            queue.drain(0..evict_count);
        }
        queue.push(Box::new(event));
    }

    /// Emit boxed event (for dynamic dispatch).
    pub fn emit_boxed(&self, event: BoxedEvent) {
        let type_id = (*event).type_id();

        // Invoke immediate callbacks
        // IMPORTANT: Use (*event).as_any() to call through dyn Event vtable,
        // not Box<dyn Event>'s blanket impl (see downcast_event docs)
        if let Some(cbs) = self.subscribers.read().unwrap_or_else(|e| e.into_inner()).get(&type_id) {
            for cb in cbs {
                cb((*event).as_any());
            }
        }

        // Queue for deferred processing with eviction
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.len() >= MAX_QUEUE_SIZE {
            let evict_count = queue.len() / 2;
            warn!("EventBus queue full ({} events), evicting oldest {}", queue.len(), evict_count);
            queue.drain(0..evict_count);
        }
        queue.push(event);
    }

    // ========== Deferred Processing ==========

    /// Poll all queued events for batch processing.
    ///
    /// Returns all events emitted since last poll. Use in main loop:
    /// ```ignore
    /// for event in event_bus.poll() {
    ///     // Process event...
    /// }
    /// ```
    pub fn poll(&self) -> Vec<BoxedEvent> {
        std::mem::take(&mut *self.queue.lock().unwrap_or_else(|e| e.into_inner()))
    }

    // ========== Handle & Utilities ==========

    /// Get an emitter handle for passing to UI components.
    pub fn emitter(&self) -> EventEmitter {
        EventEmitter {
            subscribers: Arc::clone(&self.subscribers),
            queue: Arc::clone(&self.queue),
        }
    }

    /// Clear subscribers for type E
    pub fn unsubscribe_all<E: Event>(&self) {
        self.subscribers.write().unwrap_or_else(|e| e.into_inner()).remove(&TypeId::of::<E>());
    }

    /// Clear all subscribers and queue
    pub fn clear(&self) {
        self.subscribers.write().unwrap_or_else(|e| e.into_inner()).clear();
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Check if there are subscribers for event type E
    pub fn has_subscribers<E: Event>(&self) -> bool {
        self.subscribers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&TypeId::of::<E>())
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    /// Check queue length
    pub fn queue_len(&self) -> usize {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// Lightweight emitter handle for UI components.
///
/// Can be cloned and passed to widgets for emitting events.
#[derive(Clone)]
pub struct EventEmitter {
    subscribers: Arc<RwLock<HashMap<TypeId, Vec<Callback>>>>,
    queue: Arc<Mutex<Vec<BoxedEvent>>>,
}

impl std::fmt::Debug for EventEmitter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventEmitter")
            .field("subscriber_types", &self.subscribers.read().map(|s| s.len()).unwrap_or(0))
            .field("queue_len", &self.queue.lock().map(|q| q.len()).unwrap_or(0))
            .finish()
    }
}

impl EventEmitter {
    /// Emit event: invoke callbacks and queue for deferred processing
    pub fn emit<E: Event + Clone>(&self, event: E) {
        let type_id = TypeId::of::<E>();

        // Invoke immediate callbacks
        if let Some(cbs) = self.subscribers.read().unwrap_or_else(|e| e.into_inner()).get(&type_id) {
            for cb in cbs {
                cb(&event);
            }
        }

        // Queue for deferred processing with eviction
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.len() >= MAX_QUEUE_SIZE {
            let evict_count = queue.len() / 2;
            warn!("EventEmitter queue full ({} events), evicting oldest {}", queue.len(), evict_count);
            queue.drain(0..evict_count);
        }
        queue.push(Box::new(event));
    }

    /// Emit boxed event
    pub fn emit_boxed(&self, event: BoxedEvent) {
        let type_id = (*event).type_id();

        // Invoke immediate callbacks
        // IMPORTANT: Use (*event).as_any() to call through dyn Event vtable
        if let Some(cbs) = self.subscribers.read().unwrap_or_else(|e| e.into_inner()).get(&type_id) {
            for cb in cbs {
                cb((*event).as_any());
            }
        }

        // Queue for deferred processing with eviction
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.len() >= MAX_QUEUE_SIZE {
            let evict_count = queue.len() / 2;
            warn!("EventEmitter queue full ({} events), evicting oldest {}", queue.len(), evict_count);
            queue.drain(0..evict_count);
        }
        queue.push(event);
    }
}

/// Comp-specific event emitter (wraps Option<EventEmitter>)
#[derive(Clone, Default, Debug)]
pub struct CompEventEmitter {
    inner: Option<EventEmitter>,
}

impl CompEventEmitter {
    /// Create a no-op emitter (for initialization before event system is ready)
    pub fn dummy() -> Self {
        Self { inner: None }
    }

    /// Create from EventEmitter
    pub fn from_emitter(emitter: EventEmitter) -> Self {
        Self { inner: Some(emitter) }
    }

    /// Emit event (no-op if dummy)
    pub fn emit<E: Event + Clone>(&self, event: E) {
        if let Some(ref emitter) = self.inner {
            emitter.emit(event);
        }
    }
}

/// Helper: downcast BoxedEvent to concrete type
///
/// IMPORTANT: Must explicitly deref to `dyn Event` before calling `as_any()`.
/// Without explicit deref, the blanket impl `Event for Box<dyn Event>` intercepts
/// the call and returns `&dyn Any` containing `Box<dyn Event>` instead of the
/// original type, causing downcast to always fail.
#[inline]
pub fn downcast_event<E: Event>(event: &BoxedEvent) -> Option<&E> {
    (**event).as_any().downcast_ref::<E>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};

    #[derive(Clone, Debug)]
    struct TestEvent { value: i32 }

    #[derive(Clone, Debug)]
    struct OtherEvent { msg: String }

    #[test]
    fn test_subscribe_emit_immediate() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicI32::new(0));
        let c = Arc::clone(&counter);

        bus.subscribe::<TestEvent, _>(move |e| {
            c.fetch_add(e.value, Ordering::SeqCst);
        });

        bus.emit(TestEvent { value: 10 });
        // Callback was invoked immediately
        assert_eq!(counter.load(Ordering::SeqCst), 10);

        bus.emit(TestEvent { value: 5 });
        assert_eq!(counter.load(Ordering::SeqCst), 15);
    }

    #[test]
    fn test_emit_queues_for_poll() {
        let bus = EventBus::new();

        bus.emit(TestEvent { value: 1 });
        bus.emit(TestEvent { value: 2 });
        bus.emit(OtherEvent { msg: "hello".into() });

        let events = bus.poll();
        assert_eq!(events.len(), 3);

        // Queue is empty after poll
        assert_eq!(bus.poll().len(), 0);
    }

    #[test]
    fn test_multiple_subscribers() {
        let bus = EventBus::new();
        let counter1 = Arc::new(AtomicI32::new(0));
        let counter2 = Arc::new(AtomicI32::new(0));

        let c1 = Arc::clone(&counter1);
        bus.subscribe::<TestEvent, _>(move |e| {
            c1.fetch_add(e.value, Ordering::SeqCst);
        });

        let c2 = Arc::clone(&counter2);
        bus.subscribe::<TestEvent, _>(move |e| {
            c2.fetch_add(e.value * 2, Ordering::SeqCst);
        });

        bus.emit(TestEvent { value: 10 });
        assert_eq!(counter1.load(Ordering::SeqCst), 10);
        assert_eq!(counter2.load(Ordering::SeqCst), 20);
    }

    #[test]
    fn test_emitter_handle() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicI32::new(0));
        let c = Arc::clone(&counter);

        bus.subscribe::<TestEvent, _>(move |e| {
            c.fetch_add(e.value, Ordering::SeqCst);
        });

        let emitter = bus.emitter();
        emitter.emit(TestEvent { value: 42 });

        // Immediate callback was invoked
        assert_eq!(counter.load(Ordering::SeqCst), 42);

        // Event was also queued
        assert_eq!(bus.poll().len(), 1);
    }

    #[test]
    fn test_unsubscribe() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicI32::new(0));
        let c = Arc::clone(&counter);

        bus.subscribe::<TestEvent, _>(move |e| {
            c.fetch_add(e.value, Ordering::SeqCst);
        });

        bus.emit(TestEvent { value: 10 });
        assert_eq!(counter.load(Ordering::SeqCst), 10);

        bus.unsubscribe_all::<TestEvent>();

        bus.emit(TestEvent { value: 10 });
        // Counter unchanged - no subscriber
        assert_eq!(counter.load(Ordering::SeqCst), 10);
        // But event still queued
        assert_eq!(bus.poll().len(), 2);
    }

    #[test]
    fn test_downcast() {
        let bus = EventBus::new();
        bus.emit(TestEvent { value: 42 });

        for ev in bus.poll() {
            if let Some(e) = downcast_event::<TestEvent>(&ev) {
                assert_eq!(e.value, 42);
            }
        }
    }
}
