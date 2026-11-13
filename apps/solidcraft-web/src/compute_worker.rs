#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
fn main() {
    use solidcraft_engine::forms_prototype::validation::BrowserCheck;
    use solidcraft_web::{Kind, frame, frame_bytes, read};
    use wasm_bindgen::{JsCast, closure::Closure};
    use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

    let Ok(scope) = js_sys::global().dyn_into::<DedicatedWorkerGlobalScope>() else { return };
    let reply = scope.clone();
    let receive = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let handled = (|| -> Result<Vec<u8>, String> {
            let bytes = frame_bytes(event.data()).map_err(str::to_owned)?;
            let (Kind::Check, id, input) = read(&bytes).map_err(str::to_owned)? else {
                return Err("unexpected Form CAD worker request".into());
            };
            let checked = BrowserCheck::run(input).map_err(|e| e.to_string())?;
            frame(Kind::Checked, id, &checked).map_err(str::to_owned)
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
