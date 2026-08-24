use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
    StaticSegment, WildcardSegment,
    components::{Route, Router, Routes},
    hooks::use_params_map,
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
                    <Route path=(StaticSegment("file"), WildcardSegment("path")) view=FileDetailView/>
                </Routes>
            </main>
        </Router>
    }
}

/// The operator console (spec 006): the file table + control actions (US1–US3). Every action
/// is a spec 005 control intent published by a server function — nothing the `nats` CLI can't.
#[component]
fn Console() -> impl IntoView {
    use crate::server::{cancel, pause, state_op};
    use leptos::task::spawn_local;

    let files = Resource::new(|| (), |()| crate::server::list_files());

    // Poll the file list every 3 s (client-only; the effect never runs during SSR).
    Effect::new(move |_| {
        set_interval(move || files.refetch(), std::time::Duration::from_secs(3));
    });

    // A control op runs the server function, then refetches the file list.
    let run_state = move |path: String, op: &'static str| {
        spawn_local(async move {
            let _ = state_op(path, op.to_string()).await;
            files.refetch();
        });
    };
    let run_cancel = move |job_id: String| {
        spawn_local(async move {
            let _ = cancel(job_id, false).await; // defer
            files.refetch();
        });
    };
    let do_pause = move |set: bool| {
        spawn_local(async move {
            let _ = pause(None, false, set).await; // global soft
            files.refetch();
        });
    };

    view! {
        <h1>"apsis"</h1>
        <p>
            <button on:click=move |_| do_pause(true)>"Pause"</button>
            <button on:click=move |_| do_pause(false)>"Resume"</button>
        </p>
        <Suspense fallback=|| view! { <p>"Loading…"</p> }>
            {move || Suspend::new(async move {
                match files.await {
                    Ok(rows) => view! {
                        <table>
                            <thead><tr>
                                <th>"File"</th><th>"Status"</th><th>"Why"</th><th>"Actions"</th>
                            </tr></thead>
                            <tbody>
                                {rows.into_iter().map(|r| {
                                    let path = r.path.clone();
                                    let status = r.status.clone();
                                    let job = r.job_id.clone();
                                    // Context-dependent buttons.
                                    let cancel_btn = (status == "InProgress").then(|| {
                                        let job = job.clone().unwrap_or_default();
                                        view! {
                                            <button on:click=move |_| run_cancel(job.clone())>"Cancel"</button>
                                        }
                                    });
                                    let retry_btn = (status == "Failed").then(|| {
                                        let p = path.clone();
                                        view! {
                                            <button on:click=move |_| run_state(p.clone(), "retry")>"Retry"</button>
                                        }
                                    });
                                    let requeue_btn = (status == "Done").then(|| {
                                        let p = path.clone();
                                        view! {
                                            <button on:click=move |_| run_state(p.clone(), "requeue")>"Requeue"</button>
                                        }
                                    });
                                    let p_force = path.clone();
                                    let p_done = path.clone();
                                    let href = format!("/file/{}", r.path);
                                    view! {
                                        <tr>
                                            <td><a href=href>{r.path}</a></td>
                                            <td>{r.status}{r.ignored.then_some(" (ignored)")}</td>
                                            <td>{r.decision.unwrap_or_default()}</td>
                                            <td>
                                                {cancel_btn}
                                                {retry_btn}
                                                {requeue_btn}
                                                <button on:click=move |_| run_state(p_force.clone(), "force")>"Force"</button>
                                                <button on:click=move |_| run_state(p_done.clone(), "mark_done")>"Mark done"</button>
                                            </td>
                                        </tr>
                                    }
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

/// The per-file detail view (US2): why a file is in its state — decision, last error, attempts.
#[component]
fn FileDetailView() -> impl IntoView {
    let params = use_params_map();
    // `path` is a wildcard segment, so it carries the full (possibly slashed) key.
    let detail = Resource::new(
        move || params.read().get("path").unwrap_or_default(),
        crate::server::file_detail,
    );
    view! {
        <p><a href="/">"← all files"</a></p>
        <Suspense fallback=|| view! { <p>"Loading…"</p> }>
            {move || Suspend::new(async move {
                match detail.await {
                    Ok(Some(d)) => view! {
                        <h1>{d.path}</h1>
                        <dl>
                            <dt>"Status"</dt><dd>{d.status}{d.ignored.then_some(" (ignored)")}</dd>
                            <dt>"Version"</dt><dd>{d.version}</dd>
                            <dt>"Attempts"</dt><dd>{d.attempts}</dd>
                            <dt>"Updated"</dt><dd>{d.updated_at}</dd>
                            <dt>"Job"</dt><dd>{d.job_id.unwrap_or_default()}</dd>
                            <dt>"Decision"</dt><dd>{d.decision.unwrap_or_default()}</dd>
                            <dt>"Last error"</dt><dd>{d.last_error.unwrap_or_default()}</dd>
                        </dl>
                    }.into_any(),
                    Ok(None) => view! { <p>"No such file."</p> }.into_any(),
                    Err(e) => view! { <p>"Error: "{e.to_string()}</p> }.into_any(),
                }
            })}
        </Suspense>
    }
}
