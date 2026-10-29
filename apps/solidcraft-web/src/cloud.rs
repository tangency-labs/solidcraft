//! CloudCraft from the browser build.
//!
//! Same jobs and events as the desktop worker (`solidcraft::cloud`), but the
//! client is async and the session rides the cookie the service set at
//! sign-in. There is no device flow and no token file. Compiled in only when
//! the bundle is built for a CloudCraft deployment (`SOLIDCRAFT_CLOUD_WEB`);
//! a standalone hosting such as project Pages has no service to talk to.

use craft_cloud_client::{Client, Error};
use solidcraft_ui_egui::cloud::{Account, CloudBackend, Event, Fault, GitView, Job, Post};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::spawn_local;

/// Hands cloud jobs to async tasks. Cheap to clone.
#[derive(Clone)]
pub struct Service {
    client: Client,
    ctx: egui::Context,
}

impl Service {
    /// Always available when the app is served by CloudCraft on the same
    /// origin. The page names the service; nothing is compiled in.
    pub fn new(ctx: egui::Context) -> Service {
        log::info!("cloud storage through {}", Client::from_page().base());
        Service { client: Client::from_page(), ctx }
    }
}

impl CloudBackend for Service {
    fn submit(&self, job: Job, post: Post) {
        let worker = self.clone();
        spawn_local(async move {
            worker.run(job, post).await;
        });
    }
}

impl Service {
    async fn run(self, job: Job, post: Post) {
        let post = |event: Event| {
            post.send(event);
            self.ctx.request_repaint();
        };
        match job {
            Job::Account => {
                let answer = self.me().await.map(|me| Account {
                    display: me.display,
                    capabilities: me.capabilities,
                    provider_linked: me.provider_linked,
                    git: me.git.map(|g| GitView { branch: g.branch, remote: g.remote }),
                });
                if matches!(&answer, Err(Fault::Unauthenticated)) {
                    offer_login();
                }
                post(Event::Account(answer));
            }
            Job::LinkStart => match self.client.link_start().await.map_err(fault) {
                Ok(start) => post(Event::Browser { url: start.login_url, handle: Some(start.handle) }),
                Err(fault) => post(Event::Linked(Err(fault))),
            },
            Job::LinkPoll(handle) => {
                let answer = self.client.link_poll(&handle).await.map_err(fault);
                post(Event::Linked(answer));
            }
            Job::SignOut => {
                // Reload only once the service has actually ended the session. A
                // reload after a refused sign-out would re-read the surviving cookie
                // and sign the user straight back in, having thrown away whatever was
                // open to do it. So a refusal is reported and the page is left alone.
                match fetch_logout().await {
                    Ok(()) => {
                        let _ = web_sys::window().and_then(|w| w.location().reload().ok());
                    }
                    Err(detail) => {
                        log::warn!("the sign-out did not take: {detail}");
                        post(Event::Account(Err(Fault::Refused(format!("this window could not sign out: {detail}")))));
                    }
                }
            }
            Job::List(path) => {
                let entries = self.client.list(&path).await.map_err(fault);
                post(Event::Listed { path, entries });
            }
            Job::SharedWithMe => {
                let answer = self.client.shared_with_me().await.map_err(fault);
                post(Event::Shared(answer));
            }
            Job::Read { entry, into } => {
                let id = entry.id.clone();
                let bytes = self.client.read(&id).await.map_err(fault);
                post(Event::Read { entry, into, bytes });
            }
            Job::Write { uid, entry, bytes } => {
                let id = entry.id.clone();
                let etag = entry.etag.clone();
                let answer = self.client.write(&id, &bytes, etag.as_deref()).await.map_err(fault);
                post(Event::Saved { uid, entry: answer });
            }
            Job::Create { uid, folder, name, bytes } => {
                let answer = self.client.create(&folder, &name, &bytes).await.map_err(fault);
                post(Event::Saved { uid, entry: answer });
            }
            Job::Conversation(id) => {
                let answer = self.client.conversation(&id).await.map_err(fault);
                post(Event::Conversation(answer));
            }
        }
    }

    async fn me(&self) -> Result<craft_cloud_client::Me, Fault> {
        self.client.me().await.map_err(fault)
    }
}

fn fault(e: Error) -> Fault {
    match e {
        Error::Unauthenticated => Fault::Unauthenticated,
        Error::NotLinked => Fault::NotLinked,
        Error::Unreachable(detail) => Fault::Unreachable(detail),
        Error::Refused { code, .. } if code == "conflict" || code == "409" || code == "412" => Fault::Conflict,
        Error::Refused { code, detail } => Fault::Refused(if detail.trim().is_empty() { code } else { detail }),
        Error::Malformed(detail) => Fault::Refused(format!("the cloud service gave an answer SolidCraft could not read: {detail}")),
        Error::Unsendable(detail) => {
            log::error!("cloud request could not be encoded: {detail}");
            Fault::Refused("SolidCraft could not build that request".to_owned())
        }
    }
}

/// A visitor with no session: to the service's sign-in, except on its own
/// auth pages.
fn offer_login() {
    let Some(window) = web_sys::window() else { return };
    let Ok(path) = window.location().pathname() else { return };
    if path.starts_with("/auth/") {
        return;
    }
    let _ = window.location().set_href("/auth/login");
}

async fn fetch_logout() -> Result<(), String> {
    let global = js_sys::global();
    let fetch = js_sys::Reflect::get(&global, &wasm_bindgen::JsValue::from_str("fetch"))
        .ok()
        .and_then(|v| v.dyn_into::<js_sys::Function>().ok())
        .ok_or_else(|| "no fetch".to_owned())?;
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_credentials(web_sys::RequestCredentials::Include);
    // `/auth/logout`, the same prefix `offer_login` uses. The service nests every
    // auth route under it. `/logout` is served by the static handler instead, which
    // answers a POST with 405, and `fetch` treats 405 as a perfectly good response.
    // So the old path signed nobody out: the session cookie survived and the reload
    // below put the user straight back in.
    let req = web_sys::Request::new_with_str_and_init("/auth/logout", &init).map_err(|e| format!("{e:?}"))?;
    let promised = fetch.call1(global.as_ref(), req.as_ref()).map_err(|e| format!("{e:?}"))?;
    let promised: js_sys::Promise = promised.dyn_into().map_err(|_| "not a promise".to_owned())?;
    let answered = wasm_bindgen_futures::JsFuture::from(promised).await.map_err(|e| format!("{e:?}"))?;
    // And read the status, because a resolved fetch only means the server answered.
    let response: web_sys::Response = answered.dyn_into().map_err(|_| "not a response".to_owned())?;
    if !response.ok() {
        return Err(format!("the service refused the sign-out: {}", response.status()));
    }
    Ok(())
}
