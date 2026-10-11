#[rustversion::nightly]
fn compiler_cfg() {
    println!("cargo:rustc-cfg=yew_macro_nightly");
}

#[rustversion::not(nightly)]
fn compiler_cfg() {}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(yew_macro_nightly)");
    compiler_cfg();
}
