//! apsis-web SSR server (spec 006): axum + leptos_axum. Connects to NATS once (for the
//! server functions + progress SSE) and serves the console behind the reverse proxy.

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use apsis_web::app::{App, shell};
    use apsis_web::server::ServerState;
    use axum::{Extension, Router};
    use leptos::logging::log;
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};

    // Connect to NATS once; the server functions read the KV + (later) publish control intents.
    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
    let (client, ctx) = apsis_common::connect(&nats_url)
        .await
        .expect("connect to NATS");
    let kv = ctx
        .get_key_value(apsis_common::nats::KV_BUCKET)
        .await
        .expect("open transcode_state KV");
    let state = ServerState {
        kv: apsis_common::KvStateStore::new(kv),
        client,
    };

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;
    let routes = generate_route_list(App);

    let app = Router::new()
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .layer(Extension(state))
        .with_state(leptos_options);

    log!("apsis-web listening on http://{}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // Hydration entry lives in lib.rs; the client bundle has no server main.
}
