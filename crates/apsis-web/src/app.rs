use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
    StaticSegment, WildcardSegment,
    components::{Route, Router, Routes},
    hooks::use_params_map,
};
use std::collections::{HashMap, HashSet};

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
            <Routes fallback=|| "Not found.".into_view()>
                <Route path=StaticSegment("") view=Console/>
                <Route path=(StaticSegment("file"), WildcardSegment("path")) view=FileDetailView/>
            </Routes>
        </Router>
    }
}

// ---- pure view helpers -------------------------------------------------------------------

/// Split a path into (directory-with-trailing-slash, basename).
fn split_path(p: &str) -> (String, String) {
    match p.rfind('/') {
        Some(i) => (p[..=i].to_string(), p[i + 1..].to_string()),
        None => (String::new(), p.to_string()),
    }
}

/// The library group a file belongs to — the 4th path segment (`movies`, `tv`, …), like the
/// design's `path.split('/')[3]`.
fn group_of(p: &str) -> String {
    p.split('/')
        .nth(3)
        .filter(|s| !s.is_empty())
        .unwrap_or("library")
        .to_string()
}

/// `4m12s` / `45s`.
fn eta_fmt(s: u64) -> String {
    let (m, r) = (s / 60, s % 60);
    if m > 0 {
        format!("{m}m{r:02}s")
    } else {
        format!("{r}s")
    }
}

/// Insert a space at lowercase→uppercase boundaries so `CompliantCodec` reads as two words
/// once the CSS uppercases it.
fn spaced(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev_lower = false;
    for c in s.chars() {
        if prev_lower && c.is_ascii_uppercase() {
            out.push(' ');
        }
        prev_lower = c.is_ascii_lowercase();
        out.push(c);
    }
    out
}

/// The `.ap-pill` status modifier class.
fn status_mod(status: &str) -> &'static str {
    match status {
        "Done" => "is-done",
        "InProgress" => "is-running",
        "Failed" => "is-failed",
        "Pending" => "is-pending",
        _ => "is-unknown",
    }
}

/// The `n-*` accent class for a summary count.
fn count_class(key: Option<&str>) -> &'static str {
    match key {
        None => "n-total",
        Some("Done") => "n-done",
        Some("InProgress") => "n-running",
        Some("Failed") => "n-failed",
        Some("Pending") => "n-pending",
        _ => "n-unknown",
    }
}

/// Split the persisted decision (`"CompliantCodec: hevc"`) into an uppercase key + value, and
/// flag whether it's actionable (transcode-required / not-yet-probed) so the key reads amber.
/// Falls back to a status-derived key when no decision was persisted.
fn why_parts(decision: Option<&str>, status: &str) -> (String, String, bool) {
    if let Some(d) = decision {
        let (k, v) = match d.split_once(':') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (d, ""),
        };
        let actionable = k.contains("Transcode") || k.contains("probed");
        return (spaced(k), v.to_string(), actionable);
    }
    match status {
        "Unknown" => ("not yet probed".into(), String::new(), true),
        "Failed" | "Pending" | "InProgress" => ("Transcode required".into(), String::new(), true),
        _ => ("—".into(), String::new(), false),
    }
}

/// The muted note shown in the Progress column for a non-running file.
fn idle_note(status: &str) -> &'static str {
    match status {
        "Pending" => "queued",
        "Failed" => "failed",
        "Unknown" => "not yet probed",
        _ => "—",
    }
}

// ---- inline Lucide-style icons -----------------------------------------------------------

fn i_play() -> impl IntoView {
    view! { <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor"><polygon points="6 3 20 12 6 21 6 3"></polygon></svg> }
}
fn i_pause() -> impl IntoView {
    view! { <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16"></rect><rect x="14" y="4" width="4" height="16"></rect></svg> }
}
fn i_x() -> impl IntoView {
    view! { <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M18 6 6 18M6 6l12 12"></path></svg> }
}
fn i_retry() -> impl IntoView {
    view! { <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M3 12a9 9 0 1 0 3-6.7L3 8"></path><path d="M3 3v5h5"></path></svg> }
}
fn i_requeue() -> impl IntoView {
    view! { <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M8 3 4 7l4 4"></path><path d="M4 7h11a5 5 0 0 1 0 10H9"></path></svg> }
}
fn i_probe() -> impl IntoView {
    view! { <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"></circle><path d="m20 20-3.5-3.5"></path></svg> }
}
fn i_force() -> impl IntoView {
    view! { <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M13 2 3 14h7l-1 8 10-12h-7l1-8Z"></path></svg> }
}
fn i_done() -> impl IntoView {
    view! { <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M20 6 9 17l-5-5"></path></svg> }
}
fn i_chevron() -> impl IntoView {
    view! { <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="m9 18 6-6-6-6"></path></svg> }
}

// ---- shared top bar ----------------------------------------------------------------------

/// The persistent header: wordmark + live indicator + the scheduler pause/resume toggle.
/// `paused` is optimistic local UI; the click also publishes the real spec 005 pause intent.
#[component]
fn TopBar() -> impl IntoView {
    use leptos::task::spawn_local;
    let paused = RwSignal::new(false);
    let toggle = move |_| {
        let set = !paused.get_untracked();
        paused.set(set);
        spawn_local(async move {
            let _ = crate::server::pause(None, false, set).await;
        });
    };
    view! {
        <header class="ap-topbar" class:ap-paused=move || paused.get()>
            <div class="ap-brandwrap">
                <span class="ap-brand">"apsis"<span class="ap-cursor"></span></span>
                <span class="ap-tagline">"transcode control"</span>
            </div>
            <div class="ap-spacer"></div>
            <div class="ap-live">
                <span class="ap-live-dot"></span>
                <span>{move || if paused.get() { "paused · polling held" } else { "live · refresh 3s" }}</span>
            </div>
            <button class="ap-btn ap-btn-toggle" on:click=toggle>
                {move || if paused.get() { i_play().into_any() } else { i_pause().into_any() }}
                {move || if paused.get() { "Resume scheduler" } else { "Pause scheduler" }}
            </button>
        </header>
    }
}

// ---- live progress (client-only SSE) -----------------------------------------------------

/// A live `job_id -> Progress` map, fed by the `/progress` SSE stream.
type ProgressMap = HashMap<String, crate::view::Progress>;

/// Open an `EventSource` on `/progress` and fold each frame into `progress` (client-only).
#[cfg(feature = "hydrate")]
fn subscribe_progress(progress: RwSignal<ProgressMap>) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    let Ok(es) = web_sys::EventSource::new("/progress") else {
        return;
    };
    let on_msg =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
            if let Some(txt) = e.data().as_string()
                && let Ok(p) = serde_json::from_str::<crate::view::Progress>(&txt)
            {
                progress.update(|m| {
                    m.insert(p.job_id.clone(), p);
                });
            }
        });
    es.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
    on_msg.forget();
    std::mem::forget(es);
}

// ---- the console (route `/`) -------------------------------------------------------------

#[component]
fn Console() -> impl IntoView {
    use crate::view::FileRow;
    use leptos::task::spawn_local;

    let files = Resource::new(|| (), |()| crate::server::list_files());
    let progress = RwSignal::<ProgressMap>::new(ProgressMap::new());
    let filter = RwSignal::<Option<String>>::new(None);
    let collapsed = RwSignal::<HashSet<String>>::new(HashSet::new());

    // 3s poll + live progress subscription (client-only).
    Effect::new(move |_| {
        set_interval(move || files.refetch(), std::time::Duration::from_secs(3));
        #[cfg(feature = "hydrate")]
        subscribe_progress(progress);
    });

    // Control actions: run the server function, then refetch.
    let run_state = move |path: String, op: &'static str| {
        spawn_local(async move {
            let _ = crate::server::state_op(path, op.to_string()).await;
            files.refetch();
        });
    };
    let run_cancel = move |job: String| {
        spawn_local(async move {
            let _ = crate::server::cancel(job, false).await;
            files.refetch();
        });
    };

    // One table row.
    let row_view = move |r: &FileRow| -> AnyView {
        let (dir, base) = split_path(&r.path);
        let href = format!("/file/{}", r.path);
        let pill_class = format!("ap-pill {}", status_mod(&r.status));
        let (wk, wv, actionable) = why_parts(r.decision.as_deref(), &r.status);
        let ignored = r.ignored;
        let status = r.status.clone();

        // progress cell — reactive on the SSE map, so only this cell repaints per frame.
        let prog_cell = {
            let job = r.job_id.clone();
            let status = status.clone();
            move || {
                if status != "InProgress" {
                    return view! { <span class="ap-note">{idle_note(&status)}</span> }.into_any();
                }
                match job.as_ref().and_then(|j| progress.get().get(j).cloned()) {
                    Some(p) => {
                        let denom = p.out_time_s + p.eta_s as f64 * p.speed;
                        let pct = if denom > 0.0 {
                            (p.out_time_s / denom * 100.0).clamp(0.0, 99.0)
                        } else {
                            0.0
                        };
                        let figure = format!("{:.1}× · eta {}", p.speed, eta_fmt(p.eta_s));
                        view! {
                            <div>
                                <div class="ap-bar"><div class="ap-bar-fill" style=format!("width:{pct:.0}%")></div></div>
                                <div class="ap-figure">{figure}</div>
                            </div>
                        }
                        .into_any()
                    }
                    None => view! { <span class="ap-note">"working…"</span> }.into_any(),
                }
            }
        };

        // context-dependent action buttons
        let cancel_btn = (status == "InProgress").then(|| {
            let job = r.job_id.clone().unwrap_or_default();
            view! {
                <button class="ap-btn-icon ap-btn-danger" title="Cancel job" on:click=move |_| run_cancel(job.clone())>{i_x()}</button>
            }
        });
        let retry_btn = (status == "Failed").then(|| {
            let p = r.path.clone();
            view! { <button class="ap-btn-icon" title="Retry transcode" on:click=move |_| run_state(p.clone(), "retry")>{i_retry()}</button> }
        });
        let requeue_btn = (status == "Done").then(|| {
            let p = r.path.clone();
            view! { <button class="ap-btn-icon" title="Requeue file" on:click=move |_| run_state(p.clone(), "requeue")>{i_requeue()}</button> }
        });
        let probe_btn = (status == "Unknown").then(|| {
            let p = r.path.clone();
            view! { <button class="ap-btn-icon" title="Probe now" on:click=move |_| run_state(p.clone(), "requeue")>{i_probe()}</button> }
        });
        let p_force = r.path.clone();
        let p_done = r.path.clone();

        view! {
            <tr class="ap-row">
                <td class="ap-cell-file">
                    <a class="ap-path" href=href>
                        <span class="ap-path-dir">{dir}</span><span class="ap-path-base">{base}</span>
                        {ignored.then(|| view! { <span class="ap-chip-ignored">"ignored"</span> })}
                    </a>
                </td>
                <td class="ap-cell-status">
                    <span class=pill_class><span class="ap-dot"></span>{status}</span>
                </td>
                <td class="ap-cell-progress">{prog_cell}</td>
                <td class="ap-cell-why">
                    <div class="ap-why">
                        <span class="ap-why-key" class:is-actionable=actionable>{wk}</span>
                        {(!wv.is_empty()).then(|| view! { <span class="ap-why-val">{wv}</span> })}
                    </div>
                </td>
                <td class="ap-cell-actions">
                    <div class="ap-actions">
                        {cancel_btn}
                        {retry_btn}
                        {requeue_btn}
                        {probe_btn}
                        <button class="ap-btn-icon ap-btn-danger" title="Force transcode" on:click=move |_| run_state(p_force.clone(), "force")>{i_force()}</button>
                        <button class="ap-btn-icon" title="Mark done" on:click=move |_| run_state(p_done.clone(), "mark_done")>{i_done()}</button>
                    </div>
                </td>
            </tr>
        }
        .into_any()
    };

    view! {
        <div class="ap-app">
            <TopBar/>
            <main class="ap-screen-index">
                <Suspense fallback=|| view! { <div class="ap-tablefoot">"Loading…"</div> }>
                    {move || Suspend::new(async move {
                        match files.await {
                            Ok(data) => {
                                let d_stats = data.clone();
                                let d_body = data.clone();
                                let d_foot = data.clone();
                                view! {
                                    // summary / filter strip (reactive on filter)
                                    <div class="ap-summary">
                                        {move || summary_strip(&d_stats, filter)}
                                    </div>
                                    <div class="ap-tablewrap">
                                        <table class="ap-table">
                                            <thead>
                                                <tr>
                                                    <th class="col-file">"File"</th>
                                                    <th class="col-status">"Status"</th>
                                                    <th class="col-progress">"Progress"</th>
                                                    <th class="col-why">"Why"</th>
                                                    <th class="col-actions">"Actions"</th>
                                                </tr>
                                            </thead>
                                            <tbody>
                                                // rows (reactive on filter + collapsed)
                                                {move || table_body(&d_body, filter, collapsed, &row_view)}
                                            </tbody>
                                        </table>
                                    </div>
                                    <div class="ap-tablefoot">{move || foot_note(&d_foot, filter)}</div>
                                }
                                .into_any()
                            }
                            Err(e) => view! { <div class="ap-tablefoot">"Error: "{e.to_string()}</div> }.into_any(),
                        }
                    })}
                </Suspense>
            </main>
        </div>
    }
}

/// The six filter chips — tracked (clears) + one per status, active when selected.
fn summary_strip(data: &[crate::view::FileRow], filter: RwSignal<Option<String>>) -> AnyView {
    let cur = filter.get();
    let count = |key: Option<&str>| match key {
        None => data.len(),
        Some(k) => data.iter().filter(|r| r.status == k).count(),
    };
    let chips: [(Option<&'static str>, &'static str); 6] = [
        (None, "tracked"),
        (Some("Done"), "done"),
        (Some("InProgress"), "running"),
        (Some("Failed"), "failed"),
        (Some("Pending"), "pending"),
        (Some("Unknown"), "unknown"),
    ];
    let mut out: Vec<AnyView> = Vec::new();
    for (key, label) in chips {
        let active = cur.as_deref() == key;
        let n = count(key);
        let nclass = format!("ap-stat-n {}", count_class(key));
        let key_owned = key.map(str::to_string);
        let on_click = move |_| {
            filter.update(|f| {
                if *f == key_owned {
                    *f = None;
                } else {
                    *f = key_owned.clone();
                }
            });
        };
        out.push(
            view! {
                <button class="ap-stat" class:is-active=active on:click=on_click>
                    <div class=nclass>{n}</div>
                    <div class="ap-stat-l">{label}</div>
                </button>
            }
            .into_any(),
        );
    }
    out.push(view! { <div class="ap-stat-fill"></div> }.into_any());
    if let Some(f) = cur {
        out.push(
            view! {
                <button class="ap-filter-clear" on:click=move |_| filter.set(None)>
                    {i_x()}{format!("clear filter · {f}")}
                </button>
            }
            .into_any(),
        );
    }
    out.into_iter().collect_view().into_any()
}

/// Build the table body: filter → group by library → collapsible group headers + rows.
fn table_body(
    data: &[crate::view::FileRow],
    filter: RwSignal<Option<String>>,
    collapsed: RwSignal<HashSet<String>>,
    row_view: &impl Fn(&crate::view::FileRow) -> AnyView,
) -> AnyView {
    let f = filter.get();
    let coll = collapsed.get();

    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&crate::view::FileRow>> = HashMap::new();
    for r in data
        .iter()
        .filter(|r| f.as_deref().is_none_or(|s| r.status == s))
    {
        let g = group_of(&r.path);
        if !groups.contains_key(&g) {
            order.push(g.clone());
        }
        groups.entry(g).or_default().push(r);
    }

    let mut out: Vec<AnyView> = Vec::new();
    for g in order {
        let list = &groups[&g];
        let run = list.iter().filter(|r| r.status == "InProgress").count();
        let bad = list.iter().filter(|r| r.status == "Failed").count();
        let meta = format!(
            "{} files{}{}",
            list.len(),
            if run > 0 {
                format!(" · {run} running")
            } else {
                String::new()
            },
            if bad > 0 {
                format!(" · {bad} failed")
            } else {
                String::new()
            },
        );
        let is_coll = coll.contains(&g);
        let g_toggle = g.clone();
        let label = g.clone();
        out.push(
            view! {
                <tr class="ap-group">
                    <td colspan="5">
                        <button
                            class="ap-group-btn"
                            aria-expanded=(!is_coll).to_string()
                            on:click=move |_| collapsed.update(|c| { if !c.remove(&g_toggle) { c.insert(g_toggle.clone()); } })
                        >
                            {i_chevron()}
                            <span class="ap-group-label">{label}</span>
                            <span class="ap-group-meta">{meta}</span>
                        </button>
                    </td>
                </tr>
            }
            .into_any(),
        );
        if !is_coll {
            for r in list {
                out.push(row_view(r));
            }
        }
    }
    out.into_iter().collect_view().into_any()
}

/// The footer status line under the table.
fn foot_note(data: &[crate::view::FileRow], filter: RwSignal<Option<String>>) -> String {
    let total = data.len();
    let shown = match filter.get() {
        None => total,
        Some(ref s) => data.iter().filter(|r| &r.status == s).count(),
    };
    let filtered = filter
        .get()
        .map(|f| format!(" · filtered by {f}"))
        .unwrap_or_default();
    format!("{shown} of {total} files shown{filtered} · click a path for detail")
}

// ---- the file detail (route `/file/<path>`) ----------------------------------------------

/// The per-file detail view (spec 006 US2): why a file is in its state.
#[component]
fn FileDetailView() -> impl IntoView {
    let params = use_params_map();
    let detail = Resource::new(
        move || params.read().get("path").unwrap_or_default(),
        crate::server::file_detail,
    );
    view! {
        <div class="ap-app">
            <TopBar/>
            <main class="ap-screen-detail">
                <Suspense fallback=|| view! { <p class="ap-note">"Loading…"</p> }>
                    {move || Suspend::new(async move {
                        match detail.await {
                            Ok(Some(d)) => {
                                let (dir, base) = split_path(&d.path);
                                let pill_class = format!("ap-pill {}", status_mod(&d.status));
                                view! {
                                    <a class="ap-back" href="/">"← all files"</a>
                                    <h1 class="ap-title">
                                        <span class="ap-path-dir">{dir}</span><span class="ap-path-base">{base}</span>
                                    </h1>
                                    <div class="ap-detail-rule"></div>
                                    <dl class="ap-dl">
                                        <dt>"Status"</dt>
                                        <dd class="ap-dd-pill">
                                            <span class=pill_class><span class="ap-dot"></span>{d.status}{d.ignored.then_some(" (ignored)")}</span>
                                        </dd>
                                        <dt>"Version"</dt>
                                        <dd class="ap-mono">{d.version}</dd>
                                        <dt>"Attempts"</dt>
                                        <dd class="ap-mono">{d.attempts}</dd>
                                        <dt>"Updated"</dt>
                                        <dd class="ap-mono is-dim">{d.updated_at}</dd>
                                        <dt>"Job"</dt>
                                        <dd class="ap-mono">{d.job_id.unwrap_or_else(|| "—".into())}</dd>
                                        <dt>"Decision"</dt>
                                        <dd class="ap-mono is-dim">{d.decision.unwrap_or_else(|| "—".into())}</dd>
                                        <dt>"Last error"</dt>
                                        <dd class="ap-dd-err">
                                            {match d.last_error {
                                                Some(e) => view! { <code class="ap-error">{e}</code> }.into_any(),
                                                None => view! { <span class="ap-note">"—"</span> }.into_any(),
                                            }}
                                        </dd>
                                    </dl>
                                }
                                .into_any()
                            }
                            Ok(None) => view! { <p class="ap-note">"No such file."</p> }.into_any(),
                            Err(e) => view! { <p class="ap-note">"Error: "{e.to_string()}</p> }.into_any(),
                        }
                    })}
                </Suspense>
            </main>
        </div>
    }
}
