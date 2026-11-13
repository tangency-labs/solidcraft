#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
fn main() {
    use solidcraft_engine::forms_prototype::validation::BrowserCheck;
    use solidcraft_ui_egui::forms_prototype::BrowserPreview;
    use solidcraft_web::{Kind, frame, frame_bytes, read};
    use wasm_bindgen::{JsCast, closure::Closure};
    use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

    let Ok(scope) = js_sys::global().dyn_into::<DedicatedWorkerGlobalScope>() else { return };
    let reply = scope.clone();
    let receive = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let handled = (|| -> Result<Vec<u8>, String> {
            let bytes = frame_bytes(event.data()).map_err(str::to_owned)?;
            let (kind, id, input) = read(&bytes).map_err(str::to_owned)?;
            let (kind, response) = match kind {
                Kind::Check => (Kind::Checked, BrowserCheck::run(input).map_err(|error| error.to_string())?),
                Kind::Preview => (Kind::Previewed, BrowserPreview::run(input)?),
                _ => return Err("unexpected Form CAD worker request".into()),
            };
            frame(kind, id, &response).map_err(str::to_owned)
        })();
        let output = handled.or_else(|_| frame(Kind::Failed, 0, &[]));
        if let Ok(bytes) = output
            && reply.post_message(&js_sys::Uint8Array::from(bytes.as_slice())).is_ok()
        {
            return;
        }
        reply.close();
    })
    .into_js_value();
    scope.set_onmessage(Some(receive.unchecked_ref()));
    if let Ok(ready) = frame(Kind::Ready, 0, &[])
        && scope.post_message(&js_sys::Uint8Array::from(ready.as_slice())).is_ok()
    {
        return;
    }
    scope.close();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {}
