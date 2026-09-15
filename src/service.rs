use std::sync::{Mutex, OnceLock};

/// Mirrors `aria_shell.services.aria_service.AriaService`: base for
/// process-wide, non-view-owned state/connections shared across modules
/// (e.g. a future `DisplayService` wrapping monitor add/remove signals).
///
/// Not used by Clock in this spike -- Clock doesn't use a Service in
/// Python either (it drives its own `Timer`), and a periodic tick has a
/// better native fit in iced's `Subscription` system (see
/// `main::AriaShell::subscription`). Building a Service just for the tick
/// would fight the framework. This layer is still built now, fully, per
/// the "definitive shape" requirement -- just with zero consumers so far.
pub trait Service: Sized + 'static {
    /// Called at most once, lazily, on first access (mirrors
    /// `AriaService.__init__` being called on first use via the
    /// `Singleton` metaclass).
    fn init() -> Self;

    /// Called once at app shutdown, only if `init()` was ever called
    /// (mirrors `AriaService.shutdown()` being called once at aria
    /// shutdown, not on restart).
    #[allow(dead_code)]
    fn shutdown(&mut self);
}

/// One static `ServiceCell<MyService>` per concrete service, e.g.:
/// `static DISPLAY: ServiceCell<DisplayService> = ServiceCell::new();`
/// Mirrors Python's `metaclass=Singleton` + module-level instance cache
/// in `utils/_basic.py::Singleton`, but per-type instead of a shared
/// `dyn Any` map (Rust has no single such map without paying erasure
/// costs we don't need with zero real services yet).
#[allow(dead_code)]
pub struct ServiceCell<S: Service>(OnceLock<Mutex<S>>);

#[allow(dead_code)]
impl<S: Service> ServiceCell<S> {
    pub const fn new() -> Self {
        Self(OnceLock::new())
    }

    pub fn get(&self) -> &Mutex<S> {
        self.0.get_or_init(|| Mutex::new(S::init()))
    }
}

// No global "shutdown every service that was ever created" registry is
// built yet -- deferred until a second real service exists and an actual
// shutdown-ordering need appears.
