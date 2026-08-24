use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
    StaticSegment,
    components::{Route, Router, Routes},
};

/// The SSR document shell (the `<html>` skeleton + hydration scripts).
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone() />
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    view! {
        <Stylesheet id="leptos" href="/pkg/apsis-web.css"/>
        <Title text="apsis console"/>
        <Router>
            <main>
                <Routes fallback=|| "Not found.".into_view()>
                    <Route path=StaticSegment("") view=Console/>
                </Routes>
            </main>
        </Router>
    }
}

/// The operator console (spec 006). US1: the file table. US2/US3 (decision detail, control
/// buttons) build on this.
#[component]
fn Console() -> impl IntoView {
    // A resource over the read server function; refetched on demand (later: on an interval).
    let files = Resource::new(|| (), |()| crate::server::list_files());
    view! {
        <h1>"apsis"</h1>
        <Suspense fallback=|| view! { <p>"Loading…"</p> }>
            {move || Suspend::new(async move {
                match files.await {
                    Ok(rows) => view! {
                        <table>
                            <thead><tr><th>"File"</th><th>"Status"</th><th>"Why"</th></tr></thead>
                            <tbody>
                                {rows.into_iter().map(|r| view! {
                                    <tr>
                                        <td>{r.path}</td>
                                        <td>{r.status}{r.ignored.then_some(" (ignored)")}</td>
                                        <td>{r.decision.unwrap_or_default()}</td>
                                    </tr>
                                }).collect_view()}
                            </tbody>
                        </table>
                    }.into_any(),
                    Err(e) => view! { <p>"Error: "{e.to_string()}</p> }.into_any(),
                }
            })}
        </Suspense>
    }
}
