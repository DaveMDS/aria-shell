use crate::config::ConfigSection;

/// Mirrors `aria_shell.module.GadgetRunContext(config, monitor)`.
pub struct GadgetRunContext<C> {
    pub config: C,
    /// Mirrors `monitor: Gdk.Monitor`. Simplified to the output's
    /// name/connector string for this spike -- no live monitor-object
    /// equivalent exists yet (display/output tracking is out of scope),
    /// kept as a field so the shape doesn't need to change when real
    /// output tracking is added later.
    #[allow(dead_code)]
    pub output_name: String,
}

/// Mirrors `aria_shell.module.AriaModule`.
///
/// Associated types instead of `dyn Module`/type-erasure: the module set
/// is closed and compile-time known (no dynamic plugin loading to
/// support -- see `modules::request_gadget`, which is the actual
/// "registry"), and `iced_exwlshell`'s `#[to_layer_message]` macro
/// decorates a single top-level `Message` enum, so fighting that with
/// per-module erasure/downcasting would work against the crate's
/// intended usage. iced's `Element<'_, Message>` composition idiom
/// already solves "many independently-typed children, one parent
/// message enum" cleanly via `.map(Message::Variant)`.
pub trait Module {
    type Config: ConfigSection;
    type Message: Clone + std::fmt::Debug;
    type State;

    /// Mirrors `AriaModule.gadget_factory()` (plus the "perform a first
    /// update" step done inline by `ClockModule.gadget_factory` in
    /// Python). No separate cheap-`__init__`/lazy-`module_init` split is
    /// kept here: that split existed in Python so `__init__` could raise
    /// `RuntimeError` to veto loading before `module_init` does real
    /// work -- with a closed, compiled-in module set, "can this module
    /// run at all" is a compile-time guarantee here, except where a
    /// module legitimately needs to probe the system, which Clock never
    /// does. Simplification flagged deliberately, not an oversight.
    fn gadget_factory(ctx: GadgetRunContext<Self::Config>) -> Self::State;

    fn update(state: &mut Self::State, msg: Self::Message) -> iced::Task<Self::Message>;

    fn view(state: &Self::State) -> iced::Element<'_, Self::Message>;

    /// Per-instance event source for modules that need their own (e.g. a
    /// future Workspaces module listening to a compositor IPC socket).
    /// Clock doesn't need one: the periodic tick is broadcast from a
    /// single top-level subscription in `main.rs`, exactly like Python's
    /// one `Timer` looping over every `self.gadgets` instance.
    #[allow(dead_code)] // no module in this spike needs its own event source yet
    fn subscription(state: &Self::State) -> iced::Subscription<Self::Message>;
}
