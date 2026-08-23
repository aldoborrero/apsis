//! apsis-web — the operator console (spec 006). A Leptos full-stack app: SSR from `main.rs`,
//! hydrated in the browser via [`hydrate`]. A stateless projection of the spec 005 NATS
//! surface (reads KV/progress, publishes control intents) — no new backend.

pub mod app;

/// Wasm entry: hydrate the SSR'd markup into a live Leptos app.
#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(crate::app::App);
}
