//! Native storage installation; credentials and connection setup remain separate from Git.
use solidcraft_ui_egui::Services;
mod access;
mod askpass;

pub fn askpass() -> Option<i32> {
    askpass::run()
}

pub fn install(services: &mut Services) {
    let Some(root) = solidcraft_engine::recovery::data_dir() else {
        log::error!("cloud storage: the application data directory is unavailable");
        return;
    };
    match solidcraft_engine::io::cloud::NativeHost::new(root.join("cloud")) {
        Ok(host) => services.cloud_host = Some(std::sync::Arc::new(host.with_access(std::sync::Arc::new(access::Access)))),
        Err(error) => log::error!("cloud storage: {error}"),
    }
}
