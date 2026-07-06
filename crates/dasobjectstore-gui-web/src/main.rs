#[cfg(target_arch = "wasm32")]
fn main() {
    yew::Renderer::<dasobjectstore_gui_web::App>::new().render();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("dasobjectstore-gui-web is intended to run in a WebAssembly browser target");
}
