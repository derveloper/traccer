use std::sync::{Arc, Mutex, Once};
use std::{mem};
use std::collections::HashMap;
use crate::Trace;
use rustracing::sampler::AllSampler;
use rustracing::span::SpanReceiver;
use rustracing_jaeger::span::SpanContextState;

#[derive(Clone)]
pub struct Traces {
    // Since we will be used in many threads, we need to protect
    // concurrent access
    pub(crate) inner: Arc<Mutex<HashMap<i32, Trace>>>,
}

pub fn traces() -> Traces {
    // Initialize it to a null value
    static mut SINGLETON: *const Traces = 0 as *const Traces;
    static ONCE: Once = Once::new();

    unsafe {
        ONCE.call_once(|| {
            // Make it
            let singleton = Traces {
                inner: Arc::new(Mutex::new(HashMap::new())),
            };

            // Put it in the heap so it can outlive this call
            SINGLETON = mem::transmute(Box::new(singleton));
        });

        // Now we give out a copy of the data that is safe to use concurrently.
        (*SINGLETON).clone()
    }
}

#[derive(Clone)]
pub struct Tracer {
    // Since we will be used in many threads, we need to protect
    // concurrent access
    pub inner: Arc<Mutex<(rustracing_jaeger::Tracer, SpanReceiver<SpanContextState>)>>,
}

pub fn tracer() -> Tracer {
    // Initialize it to a null value
    static mut SINGLETON: *const Tracer = 0 as *const Tracer;
    static ONCE: Once = Once::new();

    unsafe {
        ONCE.call_once(|| {
            // Make it
            let singleton = Tracer {
                inner: Arc::new(Mutex::new(rustracing_jaeger::Tracer::new(AllSampler))),
            };

            // Put it in the heap so it can outlive this call
            SINGLETON = mem::transmute(Box::new(singleton));
        });

        // Now we give out a copy of the data that is safe to use concurrently.
        (*SINGLETON).clone()
    }
}